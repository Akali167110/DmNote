import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  createIpcShim,
  serializeIpcArgs,
  type InvokeRequest,
} from '@dmnote/ipc-shim';
import { Channel, SERIALIZE_TO_IPC_FN, invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';

function fixture() {
  const requests: InvokeRequest[] = [];
  const shim = createIpcShim({
    transport: { sendInvoke: (request) => requests.push(request) },
    convertFileSrc: (path) => `asset:${path}`,
    metadata: {
      currentWindow: { label: 'main' },
      currentWebview: { windowLabel: 'main', label: 'main' },
    },
  });
  return { shim, requests };
}

afterEach(() => {
  vi.useRealTimers();
  Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
  Reflect.deleteProperty(window, '__TAURI_EVENT_PLUGIN_INTERNALS__');
  Reflect.deleteProperty(globalThis, 'isTauri');
});

describe('공통 IPC shim의 외부 전송 계약', () => {
  it('실제 Tauri invoke의 역순 응답을 연결하고 구조화된 오류를 보존한다', async () => {
    const { shim, requests } = fixture();
    shim.installGlobals(window, globalThis);
    const first = invoke('editor_get', { mode: 'a' });
    const second = invoke('editor_commit', { revision: 3 });
    const error = {
      code: 'REVISION_CONFLICT',
      details: { revision: 4 },
      retryable: true,
    };
    const rejection = expect(second).rejects.toBe(error);
    shim.receiveResponse({
      requestId: requests[1].requestId,
      ok: false,
      error,
    });
    shim.receiveResponse({
      requestId: requests[0].requestId,
      ok: true,
      result: { revision: 4 },
    });
    await rejection;
    await expect(first).resolves.toEqual({ revision: 4 });
    expect(requests.map(({ command, args }) => ({ command, args }))).toEqual([
      { command: 'editor_get', args: { mode: 'a' } },
      { command: 'editor_commit', args: { revision: 3 } },
    ]);
    shim.dispose();
  });

  it('실제 Tauri listen·emitTo·unlisten의 콜백 수명을 유지한다', async () => {
    const { shim, requests } = fixture();
    shim.installGlobals(window, globalThis);
    const callback = vi.fn();
    const unsubscribe = await listen('changed', callback);
    shim.dispatchEvent('changed', { revision: 1 });
    await emitTo('overlay', 'changed', { revision: 2 });
    expect(callback.mock.calls.map(([event]) => event.payload)).toEqual([
      { revision: 1 },
      { revision: 2 },
    ]);
    await unsubscribe();
    shim.dispatchEvent('changed', { revision: 3 });
    expect(callback).toHaveBeenCalledTimes(2);
    expect(shim.internals.callbacks.size).toBe(0);
    expect(requests).toHaveLength(0);
    shim.dispose();
  });

  it('Channel을 복제 가능한 인자로 전달하고 역순 메시지와 종료를 처리한다', async () => {
    const requests: InvokeRequest[] = [];
    const shim = createIpcShim({
      transport: {
        sendInvoke: (request) =>
          requests.push({
            ...request,
            args: structuredClone(serializeIpcArgs(request.args)),
          }),
      },
      convertFileSrc: (path) => path,
      metadata: fixture().shim.internals.metadata,
    });
    shim.installGlobals(window, globalThis);
    const callback = vi.fn();
    const channel = new Channel<string>(callback);
    const subscription = invoke('preview_subscribe', { channel });
    const request = requests[0];
    expect(request.args.channel).toBe(`__CHANNEL__:${channel.id}`);
    shim.receiveResponse({
      requestId: request.requestId,
      ok: true,
      result: null,
    });
    await subscription;
    shim.internals.runCallback(channel.id, { index: 1, message: 'second' });
    shim.internals.runCallback(channel.id, { index: 2, end: true });
    expect(callback).not.toHaveBeenCalled();
    shim.internals.runCallback(channel.id, { index: 0, message: 'first' });
    expect(callback.mock.calls).toEqual([['first'], ['second']]);
    expect(shim.internals.callbacks.has(channel.id)).toBe(false);
    shim.internals.runCallback(channel.id, { index: 2, message: 'late' });
    expect(callback).toHaveBeenCalledTimes(2);
    shim.dispose();
  });

  it('중첩한 사용자 Tauri 직렬화 훅도 저장 가능한 인자로 변환한다', () => {
    const value = { [SERIALIZE_TO_IPC_FN]: () => ({ value: 42 }) };
    expect(serializeIpcArgs({ nested: [value] })).toEqual({
      nested: [{ value: 42 }],
    });
  });

  it('호스트가 emit_to 대상을 직접 전달할 수 있다', async () => {
    const emit = vi.fn().mockResolvedValue(undefined);
    const shim = createIpcShim({
      transport: { sendInvoke: vi.fn() },
      convertFileSrc: (path) => path,
      metadata: fixture().shim.internals.metadata,
      emit,
    });
    const args = {
      target: { kind: 'Window', label: 'overlay' },
      event: 'bridge',
      payload: 1,
    };
    await shim.internals.invoke('plugin:event|emit_to', args);
    expect(emit).toHaveBeenCalledWith('plugin:event|emit_to', args);
    shim.dispose();
  });

  it('별도 shim의 응답과 이벤트는 다른 화면으로 섞이지 않는다', async () => {
    const a = fixture();
    const b = fixture();
    const callback = vi.fn();
    const id = b.shim.internals.transformCallback(callback);
    await b.shim.internals.invoke('plugin:event|listen', {
      event: 'changed',
      handler: id,
    });
    a.shim.dispatchEvent('changed', 1);
    expect(callback).not.toHaveBeenCalled();
    const pending = a.shim.internals.invoke('save');
    b.shim.receiveResponse({
      requestId: a.requests[0].requestId,
      ok: true,
      result: 'wrong',
    });
    a.shim.receiveResponse({
      requestId: a.requests[0].requestId,
      ok: true,
      result: 'right',
    });
    await expect(pending).resolves.toBe('right');
    a.shim.dispose();
    b.shim.dispose();
  });

  it('기본값은 응답을 시간으로 실패 처리하지 않고 해제 시 대기 요청을 거부한다', async () => {
    vi.useFakeTimers();
    const { shim } = fixture();
    const pending = shim.internals.invoke('save');
    const reject = expect(pending).rejects.toThrow('Disposed');
    expect(vi.getTimerCount()).toBe(0);
    vi.advanceTimersByTime(60_000);
    shim.dispose();
    await reject;
    await expect(shim.internals.invoke('save')).rejects.toThrow('Disposed');
  });

  it('동기 전송 실패를 거부하고 타임아웃 타이머를 정리한다', async () => {
    vi.useFakeTimers();
    const error = new Error('closed port');
    const shim = createIpcShim({
      transport: {
        sendInvoke: () => {
          throw error;
        },
      },
      convertFileSrc: (path) => path,
      metadata: fixture().shim.internals.metadata,
      requestTimeout: {
        milliseconds: 10_000,
        error: () => new Error('timeout'),
      },
    });
    await expect(shim.internals.invoke('save')).rejects.toBe(error);
    expect(vi.getTimerCount()).toBe(0);
    shim.dispose();
  });
});
