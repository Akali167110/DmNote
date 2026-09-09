# @dmnote/ipc-shim

Tauri JS API의 콜백·이벤트·요청 응답 처리를 호스트 전송과 분리한 패키지다. OBS WebSocket, 파일 주소, 문서 역할, 권한 목록을 패키지가 직접 알지 않는다.

## 사용 계약

- `createIpcShim(config)`를 화면마다 생성한다. `config.transport.sendInvoke`는 요청을 전송하고, 호스트가 받은 응답은 `receiveResponse`로 전달한다.
- `serializeIpcArgs(args)`는 JSON 명령 인자 안의 Tauri 직렬화 훅과 Channel 식별자를 처리한다. OBS는 기존 JSON.stringify 전송을 유지하며, MessagePort 호스트는 이 함수를 사용한 뒤 복제할 수 있다. 이 함수는 JSON 데이터용이며 바이너리 전달은 별도 호스트 계약으로 둔다. 수신한 채널 메시지 `{ index, message }`와 종료 `{ index, end: true }`는 `internals.runCallback(channelId, data)`로 전달한다. Tauri Channel이 순서 복원과 콜백 해제를 수행한다.
- 응답은 `{ requestId, ok: true, result }` 또는 `{ requestId, ok: false, error }`다. `error`는 변환하지 않으므로 편집 오류 객체도 보존한다. 기존 OBS 문자열 오류의 `Error` 변환은 OBS 어댑터에 남는다.
- `installGlobals(target, flagTarget?)`를 Tauri API 소비 코드 실행 전에 호출한다. 기존 앱은 네이티브 Tauri 연결을 계속 사용한다.
- 호스트 이벤트는 `dispatchEvent(event, payload)`로 전달한다. 콜백 식별자는 해당 shim에서만 유효하다.
- `plugin:event|emit`와 `emit_to`는 기본적으로 현재 문서에 전달한다. 다른 화면으로 전달하는 호스트는 `config.emit`을 제공해 대상·세션·전달 오류를 처리해야 한다. 기본 로컬 동작이 웹 세션 라우팅을 구현한 것은 아니다.
- `requestTimeout`을 생략하면 자동 만료하지 않는다. OBS는 기존 10초 제한을 명시적으로 설정한다. 타임아웃은 원격 실행을 취소하지 않으며 저장 성공 여부를 확정하지 않는다.
- `dispose()`는 대기 요청을 거부하고 콜백·리스너·타이머를 정리한다. 재연결 중 동일 세션을 유지할 때는 dispose하지 않고 전송 연결만 갱신한다. 종료된 shim을 재사용하지 않는다.
- 인증, allowlist, 연결 준비, 세션 등록·복구, 백엔드 이벤트 구독 준비와 자산 URL 수명은 호스트 책임이다. `requestId`는 통신 응답 연결용이며 편집 mutation ID를 대신하지 않는다.

## 빌드

루트에서 `npm run build --workspace @dmnote/ipc-shim`을 실행한다. 배포 결과는 `dist/`의 JavaScript와 선언 파일이며 프로젝트의 TypeScript 별칭이나 renderer 소스에 의존하지 않는다. 테스트는 실제 Tauri JS API를 소비하는 앱 계약 테스트 `src/renderer/api/ipcShimCore.test.ts`와 OBS 통합 테스트 `src/renderer/api/ipcShim.test.ts`에 둔다.
