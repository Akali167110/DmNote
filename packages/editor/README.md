# @dmnote/editor

DmNote 앱에서 사용하는 편집 UI, 렌더링, 플러그인 런타임과 편집 동기화 코드를 제공하는 패키지다. 소스는 현재 DmNote 레포에서 앱과 함께 관리한다. 배포 파일은 `dist/`의 JavaScript, CSS, TypeScript 선언이며 소비 프로젝트에 DmNote의 소스 별칭을 설정할 필요가 없다.

공통 구현과 인접 테스트의 실제 위치는 `packages/editor/src/`이며, `packages/editor/src/renderer/`에 편집 UI·런타임, `packages/editor/src/types/`에 공유 타입을 둔다. 패키지 최상위의 `src/*.ts`는 공개 진입점이다. 앱 전용 창·네이티브 연결은 레포 루트의 `src/`에 남는다. 공통 패키지의 선언이 앱 전용 소스에 의존하면 선언 빌드가 실패하도록 경계를 검사한다.

이 패키지만 설치한다고 저장 서버나 웹 편집 사이트가 생성되지는 않는다. Rust 공통 엔진과 환경별 저장·입력·파일·통신을 연결하는 호스트가 별도로 필요하다. 네이티브 앱은 기존 Tauri 호스트를 사용한다.

## 공개 진입점

| 경로                                             | 제공 기능                                                                                    |
| ------------------------------------------------ | -------------------------------------------------------------------------------------------- |
| `@dmnote/editor/editor`                          | Grid, PropertiesPanel, ToolBar, 다이얼로그·팝업·picker, I18nProvider 및 편집 화면 훅         |
| `@dmnote/editor/grid` / `toolbar` / `panel-host` | Grid·ToolBar·PropertiesPanelHost 기본 export. 기존 화면의 개별 import 경계를 유지하는 진입점 |
| `@dmnote/editor/dialogs`                         | 메인 다이얼로그 런타임과 수명 관리 훅                                                        |
| `@dmnote/editor/globals`                         | `window.api` 등 기존 전역의 명시적 타입 진입점 (`import type {} from ...`)                   |
| `@dmnote/editor/overlay`                         | OverlayScene, 레이아웃 계산, 오버레이 입력·노트·통계 구독 훅                                 |
| `@dmnote/editor/runtime`                         | internalApi, 호스트 전역 API 구성, bootstrap·CSS·JS 주입, 편집 coordinator와 공유 store      |
| `@dmnote/editor/install`                         | `window.api`와 플러그인 표시 레지스트리 설치, 기본 export는 internalApi                      |
| `@dmnote/editor/model`                           | 편집 문서·명령·결과 타입과 검증, 순수 편집 coordinator 생성 및 patch 처리                    |
| `@dmnote/editor/plugins`                         | 사용자 JS 런타임 생성과 플러그인 API·JS·CSS 타입                                             |
| `@dmnote/editor/style.css`                       | 편집 UI에서 사용하는 토큰·전역·컴포넌트 스타일                                               |

`install`은 의도적인 부수효과 진입점이다. `model`의 `createEditorCoordinator`는 환경별 `get`, `commit`, `onCommitted` 전송을 받아 독립된 동기화 인스턴스를 생성한다. `runtime`의 `editorCoordinator`와 store들은 기존 앱과 같은 문서 단위 공유 인스턴스다. 별도 preview 문서는 별도 store·플러그인 런타임을 유지하고 호스트 이벤트로 동기화한다.

## 호스트 연결 순서

1. React 19와 React DOM 19를 소비 프로젝트에 설치하고 동일한 React 인스턴스를 사용한다.
2. 네이티브 Tauri 연결 또는 `@dmnote/ipc-shim` 기반 호스트를 각 화면에 준비한다. 브라우저 호스트는 창 역할·자산 URL·명령 전송·이벤트 전달을 연결해야 한다. 현재 역할 계약은 `window.__dmn_window_type`의 `main` 또는 `overlay`이며 shim의 Tauri metadata와 함께 준비한다.
3. 호스트 준비가 완료된 후 `@dmnote/editor/install`을 동적으로 import한다. 창 정보를 import 시점에 읽는 API가 있으므로, 준비 전 UI·runtime·plugins 진입점을 정적으로 import하지 않는다.
4. bootstrap 결과로 defaults와 각 store를 초기화하고 편집 coordinator를 시작한다. 기존 앱의 `useAppBootstrap`과 화면 진입점이 연결 순서의 실제 사용 예다.
5. 필요한 UI 진입점과 `@dmnote/editor/style.css`를 불러와 화면에 배치한다. 메인과 오버레이의 수명·포커스·창 연결은 호스트가 관리한다.
6. 화면 종료 시 시작한 coordinator, 구독과 플러그인 런타임을 해제한다.

공통 IPC shim은 커맨드 구현체가 아니다. 호스트는 현재 공개 API의 반환값, 구조화된 오류, 준비 완료 구독과 이벤트 순서를 유지해야 한다. 지원하지 않는 명령을 `undefined` 성공으로 처리하면 안 된다. 전송 요청 ID와 편집 mutation ID·revision은 별도 계약이다.

프리셋 포맷과 플러그인 저장 의미는 기존 앱 기준을 유지한다. 브라우저 파일 선택·다운로드·IndexedDB·SharedWorker 및 외부 CSS 중계 연결은 후속 웹 호스트의 책임이다. DOM·React 요소를 직접 다루는 플러그인은 각 화면의 문서 안에서 실행한다.

## 빌드와 외부 소비 검증

레포 루트의 `npm run build:packages`로 `packages/editor/dist/`와 `packages/ipc-shim/dist/`를 빌드한다. `npm run check:packages`는 빌드 결과를 `npm pack`으로 묶고, 레포 밖의 임시 프로젝트에 tarball을 설치해 다음을 검사한다.

- 소스 별칭 없이 공개 TypeScript 선언 검사 (`skipLibCheck: false`)
- Vite의 프로젝트 설정 없이 배포된 JS·CSS와 동적 import 빌드
- 실제 공통 shim 설치 후 OverlayScene 렌더링
- 실제 편집 coordinator의 조회·commit·확정 이벤트에 따른 키 표시 변경

테스트 호스트는 필요한 계약만 구현하고 예상하지 못한 네이티브 명령을 실패 처리한다. 이 검증은 패키지 소비 경계에 대한 실행 검사이며 실제 WebView, 네이티브 저장·복구 또는 WebGL의 전체 회귀 검증을 대신하지 않는다.

패키지는 `dist/`를 포함해 배포하며 앱 내부 소스를 참조하는 상대 경로 의존성을 외부 소비자에게 요구하지 않는다. 상세 추출 범위와 후속 계획은 레포의 `docs/web-editor-design.md`에서 관리한다.
