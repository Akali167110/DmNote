import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// 프로토콜 버전 스큐 경계: 서버가 PROTOCOL_MISMATCH를 보내면
// 구 페이지처럼 조용한 재접속 루프에 빠지지 않고 즉시 종단되어야 함
class FakeWebSocket {
  static OPEN = 1;
  readyState = 1;
  static instances: FakeWebSocket[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  sent: string[] = [];

  constructor(public url: string) {
    FakeWebSocket.instances.push(this);
  }

  send(data: string) {
    this.sent.push(data);
  }

  close() {
    this.readyState = 3;
    this.onclose?.();
  }
}

describe('ipcShim 프로토콜 불일치 처리', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal('WebSocket', FakeWebSocket);
    FakeWebSocket.instances = [];
    vi.resetModules();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it('PROTOCOL_MISMATCH는 재접속 없이 즉시 종단 오류로 거부한다', async () => {
    const { initIpcShim } = await import('./ipcShim');

    const promise = initIpcShim('ws://127.0.0.1:34891', 'token');
    const socket = FakeWebSocket.instances[0];
    expect(socket).toBeDefined();

    socket.onopen?.();
    socket.onmessage?.({
      data: JSON.stringify({
        v: 2,
        type: 'error',
        seq: 0,
        ts: 0,
        payload: {
          code: 'PROTOCOL_MISMATCH',
          message: 'Unsupported protocol version',
        },
      }),
    });

    await expect(promise).rejects.toThrow('protocol mismatch');

    // 서버가 연결을 닫아도 재접속 타이머가 생성되지 않아야 함
    socket.onclose?.();
    vi.advanceTimersByTime(10_000);
    expect(FakeWebSocket.instances).toHaveLength(1);
  });

  it('AUTH_FAILED도 동일하게 종단 처리를 유지한다', async () => {
    const { initIpcShim } = await import('./ipcShim');

    const promise = initIpcShim('ws://127.0.0.1:34891', 'bad-token');
    const socket = FakeWebSocket.instances[0];

    socket.onopen?.();
    socket.onmessage?.({
      data: JSON.stringify({
        v: 2,
        type: 'error',
        seq: 0,
        ts: 0,
        payload: { code: 'AUTH_FAILED', message: 'Invalid token' },
      }),
    });

    await expect(promise).rejects.toThrow('auth failed');

    socket.onclose?.();
    vi.advanceTimersByTime(10_000);
    expect(FakeWebSocket.instances).toHaveLength(1);
  });
});

describe('OBS 전송과 공통 shim 연결', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal('WebSocket', FakeWebSocket);
    FakeWebSocket.instances = [];
    vi.resetModules();
  });

  afterEach(async () => {
    const { disposeIpcShim } = await import('./ipcShim');
    disposeIpcShim();
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    Reflect.deleteProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__');
    Reflect.deleteProperty(globalThis, 'isTauri');
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  function receive(socket: FakeWebSocket, type: string, payload: unknown) {
    socket.onmessage?.({
      data: JSON.stringify({ v: 2, type, seq: 0, ts: 0, payload }),
    });
  }

  async function connect(allowedList?: string[]) {
    const { initIpcShim } = await import('./ipcShim');
    const ready = initIpcShim('ws://localhost:34891', 'token');
    const socket = FakeWebSocket.instances[0];
    socket.onopen?.();
    receive(socket, 'hello_ack', { allowedList });
    receive(socket, 'snapshot', {});
    await ready;
    const { invoke, convertFileSrc } = await import('@tauri-apps/api/core');
    return { socket, invoke, convertFileSrc };
  }

  it('allowlist 정확 일치·응답·미디어 URL·hello 계약을 유지한다', async () => {
    const { socket, invoke, convertFileSrc } = await connect(['allowed']);
    expect(JSON.parse(socket.sent[0])).toMatchObject({
      type: 'hello',
      payload: { client: 'obs-browser', token: 'token' },
    });
    await expect(invoke('allowed_suffix')).rejects.toThrow(
      'not available in OBS mode',
    );
    expect(socket.sent).toHaveLength(1);
    const result = invoke('allowed', { value: 7 });
    const request = JSON.parse(socket.sent[1]);
    expect(request).toMatchObject({
      type: 'invoke_request',
      payload: { command: 'allowed', args: { value: 7 } },
    });
    receive(socket, 'invoke_response', {
      requestId: request.payload.requestId,
      result: 42,
    });
    await expect(result).resolves.toBe(42);
    expect(convertFileSrc('/한글/image.png')).toBe(
      `http://localhost:34891/media/${Buffer.from('/한글/image.png').toString(
        'base64url',
      )}?token=token`,
    );
    expect(vi.getTimerCount()).toBe(0);
  });

  it('allowlist 수신 전 백엔드에 위임하고 빈 목록 수신 후 차단한다', async () => {
    const { socket, invoke } = await connect();
    const result = invoke('unknown');
    const request = JSON.parse(socket.sent[1]);
    receive(socket, 'invoke_response', {
      requestId: request.payload.requestId,
      error: 'blocked by backend',
    });
    await expect(result).rejects.toThrow('blocked by backend');
    receive(socket, 'hello_ack', { allowedList: [] });
    await expect(invoke('unknown')).rejects.toThrow('not available');
    expect(socket.sent).toHaveLength(2);
  });

  it('재연결 snapshot·이벤트·ping 및 기존 10초 타임아웃을 유지한다', async () => {
    const { socket, invoke } = await connect(['allowed']);
    const { listen } = await import('@tauri-apps/api/event');
    const resync = vi.fn();
    const changed = vi.fn();
    await listen('obs:resync', resync);
    await listen('changed', changed);
    receive(socket, 'tauri_event', { event: 'changed', data: { value: 1 } });
    expect(changed).toHaveBeenCalledWith(
      expect.objectContaining({ payload: { value: 1 } }),
    );
    receive(socket, 'ping', null);
    expect(JSON.parse(socket.sent[socket.sent.length - 1])).toMatchObject({
      type: 'pong',
      payload: null,
    });
    const pending = invoke('allowed');
    const rejected = expect(pending).rejects.toThrow('RPC timeout: allowed');
    vi.advanceTimersByTime(10_000);
    await rejected;
    socket.close();
    await expect(invoke('allowed')).rejects.toThrow('WS not connected');
    vi.advanceTimersByTime(3_000);
    const next = FakeWebSocket.instances[1];
    receive(next, 'hello_ack', { allowedList: ['allowed'] });
    receive(next, 'snapshot', {});
    expect(resync).toHaveBeenCalledWith(
      expect.objectContaining({ payload: null }),
    );
  });
});
