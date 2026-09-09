/**
 * Tauri IPC Shim — OBS 환경에서 invoke/listen을 WebSocket으로 교체
 *
 * obs/index.tsx에서 앱 마운트 전에 initIpcShim()을 호출하면,
 * window.__TAURI_INTERNALS__ 및 __TAURI_EVENT_PLUGIN_INTERNALS__를 설치하여
 * overlay/App.tsx가 코드 변경 없이 동작.
 *
 * 설계 원칙 (§12.4):
 * - 커맨드별 분기 없음. 3단계만: plugin:event → allow → WS RPC
 * - allow 리스트는 hello_ack에서 수신 (백엔드가 유일한 source of truth)
 */

import { createIpcShim } from '@dmnote/ipc-shim';
import { OBS_PROTOCOL_VERSION } from '@src/types/obs';
import type { ObsEnvelope, HelloAckPayload } from '@src/types/obs';

// ── 내부 상태 ──

let ws: WebSocket | null = null;
let disposed = false;
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

// 연결 정보 (convertFileSrc에서 사용)
let connHost = '127.0.0.1';
let connPort = '34891';
let connToken = '';

// allow 리스트 — hello_ack에서 수신 (백엔드가 유일한 source of truth)
let allowList: string[] = [];
// hello_ack 수신 전에는 백엔드 이중 검사에 위임 (fail-open은 프론트 한정, 경계는 백엔드)
let allowListReceived = false;

let seqCounter = 0;
let shim: ReturnType<typeof createIpcShim>;

// ── allow 체크 ──

/** allowlist 정확 일치 — hello_ack 수신 전에는 백엔드 이중 검사에 위임 */
function isAllowed(cmd: string): boolean {
  if (!allowListReceived) return true;
  return allowList.includes(cmd);
}

// ── WS 메시지 수신 → Tauri 이벤트 디스패치 ──

function onWsMessage(envelope: ObsEnvelope) {
  switch (envelope.type) {
    // 범용 이벤트 포워딩 (§12.12)
    case 'tauri_event': {
      const { event, data } = envelope.payload as {
        event: string;
        data: unknown;
      };
      shim.dispatchEvent(event, data);
      break;
    }

    // WS RPC 응답
    case 'invoke_response': {
      const resp = envelope.payload as {
        requestId: string;
        result?: unknown;
        error?: string;
      };
      shim.receiveResponse(
        resp.error
          ? {
              requestId: resp.requestId,
              ok: false,
              error: new Error(resp.error),
            }
          : { requestId: resp.requestId, ok: true, result: resp.result },
      );
      break;
    }

    case 'snapshot': {
      // 재연결/lag 복구 시 snapshot 수신 — 내용은 버리고 재동기화 신호만 발행
      // 'obs:resync'는 shim 로컬 합성 이벤트 (백엔드 emit 아님 —
      // register_event_forwarding 등록 금지, 네이티브에서는 발화하지 않음)
      shim.dispatchEvent('obs:resync', null);
      break;
    }
  }
}

// ── WS 전송 ──

function sendWsMessage(type: string, payload: unknown = null) {
  if (!ws || ws.readyState !== WebSocket.OPEN) return;
  const envelope: ObsEnvelope = {
    v: OBS_PROTOCOL_VERSION,
    type,
    seq: seqCounter++,
    ts: Date.now(),
    payload,
  };
  ws.send(JSON.stringify(envelope));
}

// ── convertFileSrc shim ──

function shimConvertFileSrc(filePath: string, _protocol = 'asset'): string {
  // OBS HTTP 서버의 /media/ 엔드포인트로 변환
  // 백엔드가 base64url(no-pad)을 기대하므로 표준 base64 → base64url 변환
  const bytes = new TextEncoder().encode(filePath);
  const binary = Array.from(bytes, (b) => String.fromCharCode(b)).join('');
  const encoded = btoa(binary)
    .replace(/\+/g, '-')
    .replace(/\//g, '_')
    .replace(/=+$/, '');
  const tokenParam = connToken ? `?token=${connToken}` : '';
  return `http://${connHost}:${connPort}/media/${encoded}${tokenParam}`;
}

// ── 공개 API ──

/**
 * IPC shim 초기화. WS 연결 → hello_ack(allowList 수신) → snapshot 수신 → 글로벌 설치.
 * 반드시 dmnoteApi import 전에 호출.
 */
export function initIpcShim(wsUrl: string, token: string): Promise<void> {
  disposed = false;

  // 연결 정보 파싱 (convertFileSrc에서 사용)
  try {
    const url = new URL(wsUrl);
    connHost = url.hostname || '127.0.0.1';
    connPort = url.port || '34891';
  } catch {
    connHost = '127.0.0.1';
    connPort = '34891';
  }
  connToken = token;
  shim = createIpcShim({
    transport: {
      sendInvoke: (request) => {
        // hello_ack의 정확 일치 검사, 최종 권한 경계는 기존 OBS 백엔드
        if (!isAllowed(request.command)) {
          throw new Error(
            `[IPC Shim] command is not available in OBS mode: ${request.command}`,
          );
        }
        if (!ws || ws.readyState !== WebSocket.OPEN) {
          throw new Error(`[IPC Shim] WS not connected: ${request.command}`);
        }
        sendWsMessage('invoke_request', request);
      },
    },
    convertFileSrc: shimConvertFileSrc,
    metadata: {
      currentWindow: { label: 'obs-overlay' },
      currentWebview: { windowLabel: 'obs-overlay', label: 'obs-overlay' },
    },
    requestTimeout: {
      milliseconds: 10000,
      error: (command) => new Error(`[IPC Shim] RPC timeout: ${command}`),
    },
  });

  return new Promise((resolve, reject) => {
    let resolved = false;

    const connect = () => {
      if (disposed) return;

      ws = new WebSocket(wsUrl);

      ws.onopen = () => {
        sendWsMessage('hello', {
          client: 'obs-browser',
          protocol: OBS_PROTOCOL_VERSION,
          appVersion: '',
          resumeFromSeq: 0,
          token: token || undefined,
        });
      };

      ws.onmessage = (event) => {
        let envelope: ObsEnvelope;
        try {
          envelope = JSON.parse(event.data as string) as ObsEnvelope;
        } catch {
          return;
        }

        if (envelope.type === 'hello_ack') {
          // allow 리스트 수신 (없으면 기본값 유지)
          const payload = envelope.payload as HelloAckPayload;
          if (payload.allowedList) {
            allowList = payload.allowedList;
            allowListReceived = true;
          }
          return;
        }

        if (envelope.type === 'ping') {
          sendWsMessage('pong');
          return;
        }

        if (envelope.type === 'error') {
          const payload = envelope.payload as Record<string, unknown>;
          // 종단 오류: 재접속으로 해소되지 않으므로 재시도 없이 즉시 중단
          if (
            payload?.code === 'AUTH_FAILED' ||
            payload?.code === 'PROTOCOL_MISMATCH'
          ) {
            disposed = true;
            if (!resolved) {
              resolved = true;
              reject(
                new Error(
                  payload.code === 'AUTH_FAILED'
                    ? 'OBS auth failed'
                    : 'OBS protocol mismatch - refresh the browser source',
                ),
              );
            }
          }
          return;
        }

        // snapshot 수신 시 글로벌 설치 후 resolve
        if (envelope.type === 'snapshot' && !resolved) {
          shim.installGlobals(window, globalThis);
          resolved = true;
          resolve();
          return;
        }

        // 이후 메시지는 이벤트로 디스패치
        onWsMessage(envelope);
      };

      ws.onclose = () => {
        ws = null;
        if (!disposed) {
          reconnectTimer = setTimeout(connect, 3000);
        }
      };

      ws.onerror = () => {
        // onclose에서 처리
      };
    };

    // 초기 연결 타임아웃 15초
    const initTimeout = setTimeout(() => {
      if (!resolved) {
        resolved = true;
        reject(new Error('[IPC Shim] Connection timeout'));
      }
    }, 15000);

    const originalResolve = resolve;
    resolve = (value) => {
      clearTimeout(initTimeout);
      originalResolve(value);
    };

    connect();
  });
}

/** shim 해제 */
export function disposeIpcShim() {
  disposed = true;

  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }

  if (ws) {
    ws.close();
    ws = null;
  }

  shim?.dispose();
  allowList = [];
  allowListReceived = false;
}
