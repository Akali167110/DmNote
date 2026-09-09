# 웹 편집 엔진 구현 기록

`feat/web-editor-host`는 [설계 결정](web-editor-design.md)의 2026-09-09 후속 범위를 구현한다. 소스는 DmNote에서 관리하고 사이트는 배포된 패키지를 소비한다. 패키지 설치와 연결 예제는 [web-host README](../packages/web-host/README.md)에 둔다.

## 소비 경계

| 단위                   | 책임                                                                                           |
| ---------------------- | ---------------------------------------------------------------------------------------------- |
| `@dmnote/editor`       | Grid·패널·공유 렌더러, 편집 coordinator, CSS·JS 플러그인 런타임과 기존 API                     |
| `@dmnote/ipc-shim`     | Tauri JS 호환 invoke·이벤트·Channel·콜백 계약                                                  |
| `@dmnote/web-host`     | SharedWorker 연결, 세션·큐·영속 저장, 브라우저 파일·입력·자산 URL·소리와 미리보기 상태         |
| `dmnote-editor-engine` | Rust 편집·history 전이와 검증, 설정·탭·플러그인 저장, 프리셋과 자산 변환, 키 매칭·preview 검증 |
| 사이트                 | 문서 선택 UI, 메인 화면과 overlay iframe 조립, CSS 중계 URL·정적 자산·사이트 배포              |

`npm run build:packages`는 editor·shim 및 웹 호스트를 빌드한다. 웹 호스트의 JS, wasm-bindgen web 바인딩과 `.wasm`을 같은 npm 산출물에 넣는다. `worker.js`와 `wasm/`의 상대 위치를 유지해 정적 배포하면 된다. 데스크톱의 `npm run tauri:dev`와 빌드는 기존 source alias를 그대로 사용하며 WASM 빌드를 선행 조건으로 추가하지 않는다.

## 실행 세션과 통신

한 origin의 SharedWorker가 `documentId`별 세션을 소유한다. 문서당 `main` 하나와 `overlay` 하나를 허용하며 같은 client ID 재연결은 이전 port를 교체한다. 각 iframe·탭은 shim을 설치한 다음 editor 모듈을 동적으로 가져온다. DOM·React 플러그인은 해당 문서 안에서 실행한다.

`connect → ready → invoke/response, event, callback` 프로토콜은 [protocol.ts](../packages/web-host/src/protocol.ts)에 둔다. 등록된 port에서 세션·역할을 결정하고 요청 인자로 다른 문서를 선택하지 못하게 한다. plugin bridge의 대상은 같은 세션의 역할로 해석한다. main만 plugin authority·instances·gesture 변경을 확정할 수 있다.

SharedWorker는 정확히 같은 origin에서 연결된다. Web Locks로 문서 소유권을 잡아 서로 다른 worker URL·버전이 같은 문서를 동시에 소유하지 못하게 한다. IndexedDB의 version 비교도 별도로 유지한다. 브라우저 문서가 닫히면 해제되는 client lease와 sessionStorage ID로 정상 재연결과 복제 탭을 구분한다. [SharedWorker 문서](https://developer.mozilla.org/en-US/docs/Web/API/SharedWorker), [Web Locks 표준](https://www.w3.org/TR/web-locks/).

## 저장·복구와 재전송

1. 세션 큐 안에서 Rust가 변경 후보·checkpoint·결과·이벤트·자산 변경을 준비한다.
2. IndexedDB `documents`, `assets`, `receipts`를 같은 `readwrite` 트랜잭션으로 쓴다. version 비교가 실패하면 덮어쓰지 않는다.
3. `complete` 이후에만 Rust confirm과 자산 URL 전달, 확정 이벤트, 성공 응답을 진행한다. 실패 시 discard하며 IO 오류는 기존 편집기의 재시도 가능한 `IO_ERROR` 형태를 유지한다.

트랜잭션에는 `durability: 'strict'`를 지정한다. 요청 하나의 성공 이벤트가 아니라 전체 트랜잭션 완료를 저장 완료로 판단한다. [IndexedDB 트랜잭션 문서](https://developer.mozilla.org/en-US/docs/Web/API/IDBDatabase/transaction).

receipt에는 요청 ID·요청 fingerprint·결과를 저장한다. 응답 유실 후 같은 요청이 재전송되면 이미 확정한 변경을 반복하지 않는다. 다른 내용에 같은 ID를 쓰면 거절한다. 입력 애니메이션·일반 bridge 이벤트는 영속 편집 요청이 아니며 저장 receipt 대상이 아니다. 키 카운터는 문서와 checkpoint를 저장하지만 매 키 입력에 영구 receipt를 추가하지 않는다.

checkpoint는 plugin authority/model revision, 카운터 세션·revision 및 history 메타를 보존한다. Worker 재시작 시 undo payload는 복원하지 않고 history sequence와 epoch를 전진시킨다. 이미 성공한 undo 요청의 재전송은 다시 실행하지 않고 현재 history 상태를 돌려준다. 브라우저가 저장소를 삭제한 경우나 origin이 변경된 경우에는 이 저장 상태를 복원할 수 없다.

손상 복구는 문서의 `incarnation`을 바꾸고 원본 문서·checkpoint를 별도 복구 필드에 보존한다. 이전 incarnation의 receipt는 성공 응답으로 재사용하거나 변경을 반복 적용하지 않고 revision conflict로 돌려 화면을 재동기화한다. 정상 Worker 재시작에서는 incarnation을 유지하므로 확정 요청을 그대로 재생할 수 있다.

자산 삭제는 30일 격리다. 삭제된 자산은 활성 목록에서 제외하되 바이트를 즉시 지우지 않는다. 실제 정리는 세션의 마지막 화면이 해제되어 history가 종료된 뒤 수행하고 현재·직전 저장 문서가 참조하는 자산을 보호한다. Rust의 손상 복구가 발생한 세션은 정리를 건너뛴다. 직전 문서와 receipt는 사용자가 내보내는 프리셋에 추가하지 않는다.

## 기존 API와 브라우저 어댑터

프리셋은 기존 `PresetFile`과 asset reader/writer를 사용한다. 웹에서 가져올 파일은 브라우저가 선택하고, JSON 해석·내장 자산 검증·폰트 메타데이터·사운드 및 문서 전이는 공통 Rust에서 처리한다. `plugin.storage`의 이름공간·instances 제약과 프리셋 미포함 규칙도 유지한다.

CSS·JS의 로컬 파일 감시 대신 가져온 원본 바이트를 저장소에 보관하고 reload·history에서 읽는다. 외부 CSS import는 사이트가 설정한 중계 fetcher를 사용한다. 실제 CSS 스코프 처리와 플러그인 실행은 editor 패키지의 기존 런타임이 담당한다.

preview의 필드·크기·UUID 검증은 공통 Rust를 호출한다. host broker는 port별 Channel, 단조 seq, session 소유권, 종료된 session의 tombstone을 관리한다. undo/redo는 큐 밖에서 모든 화면에 기존 `app:close-requested {action:'history'}`를 보내 미확정 편집을 저장하게 하고, 전원 확인 후 큐에서 실행한다. 실패·연결 종료 시 잠금을 해제한다.

키 입력은 브라우저 이벤트를 공통 Rust matcher로 전달한다. blur·pagehide·연결 교체 시 해당 화면이 누른 키를 해제한다. Web Audio 어댑터는 첫 사용자 동작 이후 소리를 재생하며, 한 입력이 여러 슬롯을 활성화해도 네이티브와 같은 슬롯 우선순위로 소리를 한 번 선택한다. 키음은 입력을 받은 화면 한 곳에서만 재생한다.

업데이트·앱 종료·OS 창 조작·OBS 서버·네이티브 오디오 장치 선택은 브라우저 호스트의 지원 기능이 아니다. 명시적인 오류로 반환한다. overlay의 화면 안 크기·위치·가시성·페이드는 호스트 상태로 제공해 사이트가 미리보기 컨테이너에 반영할 수 있게 한다.

## 검증

실행 명령:

```sh
npm run setup:web-wasm  # 최초 도구 준비, Cargo.lock과 CLI 버전 동기화
npm run build:packages
npm run check:packages
npm run check:web-host
npm run type-check
npm test
cargo test --manifest-path src-tauri/Cargo.toml --workspace
```

`npm run build:editor`는 Rust/WASM 없이 editor·shim의 TypeScript 산출물만 만든다. `npm run type-check`는 타입 선언을 먼저 생성하므로 dist가 없는 깨끗한 체크아웃에서도 별도 WASM 빌드 없이 실행할 수 있다.

`check:web-host`는 세 패키지를 실제 tarball로 묶어 임시 외부 프로젝트에 설치하고 Chrome에서 WASM·SharedWorker·IndexedDB·메인/overlay 통신과 공유 UI를 검사한다. 기본 Chrome 외 경로는 `DMNOTE_BROWSER_PATH`로 지정한다. 사이트 배포, 외부 CSS 중계 서버 배포, 각 OS WebView의 수동 검증은 이 검사와 구분한다.

2026-09-09 최종 실행 결과:

- 프론트엔드 전체: 458개 파일, 4,727개 테스트 통과, 18개 테스트 skip.
- Rust workspace: 앱 852개 통과·6개 ignore, 공통 엔진 349개 통과. 실제 WASM Node smoke도 통과.
- 외부 패키지 소비: source alias 없이 타입 검사·번들·실행 통과. 웹 소비자의 타입 검사는 `skipLibCheck: false`로 실행.
- 실제 Chromium: 문서 격리, 편집 확정, preview Channel, history flush, plugin storage·bridge·DOM, 자산 포함 프리셋 왕복, 키 입력, 재접속, CSS, 공유 Grid 조립, 손상 문서·checkpoint 복구 통과.
- 타입 검사·포맷·린트, Rust workspace check·strict clippy·fmt, editor·web-host 패키지 빌드와 데스크톱 프론트엔드 빌드 통과. 린트에는 기존 `SoundTrimModal` 경고 한 건이 남아 있다.
- dist가 없는 별도 체크아웃의 타입 검사, CI 정책 테스트 15개와 actionlint 통과. 원격 CI 실행과 OS별 WebView·OBS 수동 검증은 수행하지 않았다.

사이트 UI 조립·배포와 외부 CSS 중계 서버 배포는 사이트 저장소의 후속 작업이다. npm 공개 배포는 수행하지 않았으며, 위 외부 소비 검사는 로컬 tarball을 사용한다.
