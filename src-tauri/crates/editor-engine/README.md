# dmnote-editor-engine

DmNote 데스크톱과 웹 호스트가 공유하는 Rust 편집 엔진. Rust 1.88 이상이 필요하며 네이티브와 `wasm32-unknown-unknown`으로 빌드한다. Tauri, 파일 I/O, 네이티브 윈도우·전역 입력 수집 및 저장 작업의 동기화는 포함하지 않는다.

## 공개 계약

- `models`: 기존 문서·편집·플러그인·설정 wire 형식과 정규화.
- `commit`: 요청 정규화, mutation 재전송과 revision 검사, patch/ops 준비 및 저장 성공 이후 확정. 데스크톱과 JSON 세션이 같은 함수를 사용한다.
- `state::history`, `history_transition`: undo/redo 기록·복원 후보와 저장 실패 시 barrier 상태. 데스크톱과 웹이 동일한 복원 알고리즘을 소비하며 네이티브 admission lock은 앱에 남는다.
- `state::{editor, editor_ops, gesture, plugin, native_element_id}`: 편집·플러그인 검증과 데이터 전이, ID 처리.
- `preset::{plan, assets, export, validation}`: 전체/탭 프리셋 계획, 내장 자산 변환과 JSON 해석. `PresetAssetReader`, `PresetAssetWriter`, `PresetImportHost`가 실제 자산 I/O와 CSS 경로 검증을 제공한다.
- `state::migration::load_store_bytes`: 바이트 입력의 항목별 복구·마이그레이션 판단. 시각과 레거시 CSS 경로 정규화는 호출자가 제공한다. 파일 복구·백업·자산 격리는 호스트가 실행한다.
- `settings`: 설정 적용과 차이 계산.
- `session`: `EditorSession`의 JSON 호출 경계와 WASM 바인딩. strict editor prepare/confirm/discard, snapshot 및 mutation 재전송을 검증하는 소비 예제다. 기존 경량 소비자 계약을 유지한다.
- `web_session`: `WebEditorSession`의 편집·gesture·history·settings·tab·plugin storage·CSS/JS·프리셋/자산 명령 dispatcher. `web_command_kind`가 지원하는 명령의 읽기/쓰기 구분을 제공한다.
- `web_preset`, `web_resources`: 논리 자산 키와 base64 입력의 검증·프리셋 변환·이미지/폰트/사운드 처리 후보.
- `keyboard`: 네이티브와 WASM `WebKeyboardMatcher`가 공유하는 multi-key 매칭과 물리 입력 중복 억제.
- `preview`: 네이티브와 WASM이 공유하는 preview payload·UUID·필드·크기 검증. 연결·구독·전송은 각 호스트가 소유한다.

호스트는 준비와 확정 사이에 같은 세션의 다른 변경을 실행하지 않는다. `prepare_editor_commit`으로 생성된 `pending_store()`를 먼저 저장하고 저장 성공 후 `finalize()`를 호출한다. 저장 실패 시 준비 결과를 버리고 확정 상태와 히스토리를 유지한다. `finalize`는 저장 성공 여부를 직접 확인하지 않으므로 호출 순서를 호스트가 보장해야 한다.

`WebEditorSession.prepare_command(command, args_json, token)`은 `{token, store, checkpoint, result, events, assetWrites, assetDeletes}`를 반환한다. 호스트는 문서·checkpoint·자산·RPC receipt를 원자적으로 저장한 다음 `confirm_command(token)`을 호출한다. confirm은 준비했던 JSON과 같은 응답을 반환한다. 저장 실패 시 `discard_command(token)`으로 문서·history·authority·ack를 모두 유지한다. `store: null`이어도 checkpoint와 receipt는 저장해야 한다. 재시작 시 생성 직후 `restore_checkpoint(checkpoint_json)`으로 authority와 counter revision을 복원한다. undo 기록은 실행 세션에만 남는다.

자산 trait의 `Path`는 호스트가 해석하는 논리적 참조다. 웹은 브라우저 자산 키를 매핑하고, 가져온 바이트를 메모리에 준비한 후 문서와 함께 저장할 수 있다. 엔진은 경로를 직접 열지 않는다. `local_asset_path`는 과거 앱의 파일 참조를 해석하는 순수 호환 도우미이며 웹 파일 접근 권한을 부여하지 않는다.

## 검증

```sh
cargo test --manifest-path src-tauri/Cargo.toml --workspace
cargo build --manifest-path src-tauri/Cargo.toml -p dmnote-editor-engine --target wasm32-unknown-unknown
wasm-bindgen --target nodejs --out-dir src-tauri/target/wasm-node src-tauri/target/wasm32-unknown-unknown/debug/dmnote_editor_engine.wasm
node --experimental-strip-types src-tauri/crates/editor-engine/tests/wasm-smoke.ts src-tauri/target/wasm-node/dmnote_editor_engine.js
```

`wasm-bindgen-cli` 버전은 `Cargo.lock`의 `wasm-bindgen`과 일치시킨다. 실제 웹 배포용 바인딩은 같은 `.wasm`을 `wasm-bindgen --target web`으로 생성할 수 있다. Node smoke는 컴파일된 WASM의 호출과 저장 전후 상태를 검사하며 브라우저·SharedWorker 제품 구현 전체를 검증하는 테스트는 아니다.

`test-support` feature는 데스크톱 회귀 테스트가 히스토리 용량 한계와 복구 보조 함수를 검증하는 용도다. 프로덕션 소비자는 활성화하지 않는다. 기본 프리셋 JSON과 공통 단위 테스트 fixture는 패키지에 포함된다.

라이선스: GPL-3.0-only. 기존 DmNote 코드의 라이선스를 유지한다.
