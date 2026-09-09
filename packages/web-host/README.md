# @dmnote/web-host

DM Note 공통 편집 패키지의 브라우저 호스트입니다. SharedWorker가 문서별 Rust/WASM 실행 세션·IndexedDB 저장·main/overlay 통신을 소유하고, 각 브라우저 문서는 기존 Tauri 호환 API를 통해 편집기를 사용합니다. 사이트 화면 구성·로그인·배포·CSS 중계 서버는 소비 웹 레포의 책임입니다.

## 설치와 시작 순서

`@dmnote/editor`, `@dmnote/ipc-shim`, `@dmnote/web-host`의 대응 버전을 함께 사용합니다. 호스팅 경로 `/engine/`에 web-host 패키지의 `dist/` 내용 전체를 복사합니다. `/engine/worker.js`와 `/engine/wasm/`의 상대 경로를 유지하고 `.wasm`은 `application/wasm`으로 제공합니다. HTTPS 또는 localhost에서 SharedWorker, Web Locks, IndexedDB를 지원하는 브라우저가 필요합니다.

```ts
import { connectWebEditor, createCssImportFetcher } from '@dmnote/web-host';

const host = await connectWebEditor({
  workerUrl: new URL('/engine/worker.js', location.href),
  documentId: 'my-preset',
  role: 'main',
  fetchCssImport: createCssImportFetcher({
    proxyUrl: new URL('/api/css-import', location.href),
  }),
});

await host.installEditorApi();
const runtime = await import('@dmnote/editor/runtime');
const editor = await import('@dmnote/editor/editor');
// 이 이후에 runtime 훅과 editor 컴포넌트로 사이트의 편집 화면 구성
```

`connectWebEditor`는 worker가 문서와 자산을 준비한 뒤 `__TAURI_INTERNALS__`, `__dmn_runtime = 'web'`, `__dmn_window_type`을 설치합니다. `@dmnote/editor/install`, runtime, UI는 위 순서대로 동적 import합니다. 한 브라우저 문서에는 한 호스트만 설치합니다.

동일 `documentId`의 main과 overlay는 같은 세션을 공유합니다. 각 역할은 한 클라이언트만 연결할 수 있고, 다른 `documentId`의 상태와 플러그인 저장소는 분리됩니다. 같은 사이트의 별도 iframe에서 `role: 'overlay'`로 연결해 미리보기를 조립할 수 있습니다. iframe의 DOM/CSS 경계·크기·보안 정책은 소비 사이트가 설정합니다.

## 연결과 저장 경계

`host.invoke(command, args)`는 기존 명령을 호출합니다. 일반 편집 코드는 설치된 `window.api` 또는 공통 editor coordinator를 사용하면 됩니다. 저장 응답은 IndexedDB 트랜잭션 완료와 Rust 상태 확정 뒤 전달됩니다. 자산은 참조 이벤트·응답 전에 Blob URL로 등록됩니다.

`host.subscribeConnectionState(listener)`와 `host.connectionState`로 연결 상태를 표시합니다. 실패한 연결은 `await host.reconnect()`로 복구합니다. 완료 여부를 알 수 없는 요청은 같은 요청 ID로 재전송하며, 저장 receipt로 이미 완료된 mutation을 다시 적용하지 않습니다. 편집 mutation은 단순 시간 경과로 실패시키지 않습니다. `connectTimeoutMs`는 세션 연결 준비에만 적용됩니다.

기본 client ID는 sessionStorage에 저장해 reload 뒤 기존 역할을 다시 연결합니다. Web Locks lease는 sessionStorage를 복제한 새 탭이 기존 탭의 ID를 차지하지 않도록 분리합니다. `clientId`를 직접 전달하면 ID의 수명과 의도적인 연결 교체를 소비 사이트가 관리해야 합니다. `pagehide`는 역할을 해제하며, bfcache의 `pageshow`는 같은 ID로 재연결합니다. Channel과 raw input 구독은 재연결 뒤 복구됩니다. 같은 페이지의 포트 재연결은 플러그인 실행 권한을 유지합니다. 새 페이지 연결이나 bfcache 복귀처럼 이전 main 실행이 종료된 경계에서는 이전 권한을 회수하고 공통 JS 런타임을 다시 초기화합니다.

`host.dispose()`는 포트·리스너·오디오·Blob URL을 정리하고 대기 요청을 거부합니다. **대기 중인 편집 저장을 기다리는 함수가 아닙니다.** 사이트의 닫기 동작에서는 포커스된 입력과 저장 barrier를 먼저 정산합니다.

```ts
const { flushFocusedEditor, runAfterEditorFlush } = await import(
  '@dmnote/editor/runtime'
);

if (!(await flushFocusedEditor())) {
  throw new Error('편집 내용을 확정하지 못했습니다.');
}
await runAfterEditorFlush('web editor close', async () => {
  host.dispose();
});
```

브라우저 자체 종료 이벤트에서는 비동기 저장 완료를 보장할 수 없습니다. 사이트는 편집 중 정상 저장 결과와 오류를 표시하고, 자체 화면 전환에서는 위 종료 경계를 사용해야 합니다.

## 브라우저 어댑터

- 키보드·마우스: 기본 활성화. 현재 브라우저 문서의 키와 MOUSE1–5 입력을 공통 Rust matcher로 보내고 blur·비활성화 시 눌린 키를 해제합니다. `keyboard: false`로 기본 연결을 끄거나 `attachBrowserKeyboard`를 별도로 조립할 수 있습니다. 운영체제 전역 입력은 수집하지 않습니다.
- 오디오: 기본 활성화. 입력을 받은 클라이언트에서 `web:input-sound`를 Web Audio로 재생합니다. 실제 사용자 keydown/pointerdown으로 브라우저 오디오를 활성화합니다. `audio: false`로 끌 수 있으며 실패는 `web:error` 이벤트로 전달됩니다.
- 오버레이: `onOverlayState(state)`로 미리보기 크기·위치·표시·잠금·opacity를 받습니다. `applyWebOverlayState(iframeOrWrapper, state)`를 사용하면 `position: relative`인 부모 안에 미리보기 영역을 배치합니다. iframe의 콘텐츠 내부 host와 부모의 배치는 사이트가 연결합니다.
- 파일: 이미지·폰트·사운드·CSS·JS와 프리셋 불러오기는 브라우저 picker를 사용합니다. 선택 파일은 bytes로 worker에 전달되고, 프리셋 JSON은 기존 자산 포함 형식을 유지합니다. 취소 결과는 앱 계약을 유지합니다. `pickFiles`/`download` 주입으로 사이트의 파일 UI와 다운로드 방식을 바꿀 수 있습니다.
- 자산: `host.assets.resolve(path)` 또는 기존 `convertFileSrc`로 Blob URL을 얻습니다. 교체된 URL은 기존 렌더 참조를 위해 host 종료까지 유지합니다. 자산 등록은 `AssetUrlRegistry`로 독립 사용 가능합니다.
- CSS import: `fetchCssImport`로 중계 호출을 주입합니다. 기본 helper는 HTTP(S) URL을 `?url=`로 보내고 `{finalUrl, text}` JSON을 받습니다. redirect 후 상대 import 기준을 유지합니다. 중계 서버의 외부 접근 정책·배포는 사이트 소유입니다. helper는 5초 네트워크 제한과 1MiB CSS 본문 제한을 적용합니다.

창 최소화·네이티브 패널 분리·앱 종료/업데이트·시스템 오디오 장치 선택 등 네이티브 전용 명령은 명시적으로 거절합니다. `app_quit_after_editor_flush`는 웹에서도 history flush 확인 응답이므로 전달되며, 실제 앱 종료 기능을 제공하지 않습니다.

추가 public API: `createWebAudioPreview`, `pickBrowserFiles`, `downloadBrowserFile`, `createCssImportFetcher`, `AssetUrlRegistry`, `attachBrowserKeyboard` 및 각 옵션/결과 타입. 브라우저 어댑터를 별도로 조립하는 경우 설치와 해제의 소유자는 소비 사이트입니다.
