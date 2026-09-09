import { MessageChannel, type MessagePort } from 'node:worker_threads';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { Channel, invoke } from '@tauri-apps/api/core';
import { emitTo, listen } from '@tauri-apps/api/event';
import { connectWebEditor, type WebEditorClient } from './connectWebEditor';
import type { PortLike, WebClientMessage, WebServerMessage } from '../protocol';

const clients: WebEditorClient[] = [];
const ports: MessagePort[] = [];
const connections: Array<{ port: MessagePort; messages: WebClientMessage[] }> =
  [];
const notify = <T>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
};
function factory(
  handler?: (
    message: WebClientMessage,
    port: MessagePort,
    index: number,
  ) => void,
) {
  return () => {
    const pair = new MessageChannel();
    ports.push(pair.port1, pair.port2);
    const index = connections.length;
    const current = { port: pair.port2, messages: [] as WebClientMessage[] };
    connections.push(current);
    pair.port2.on('message', (message: WebClientMessage) => {
      current.messages.push(message);
      if (message.type === 'connect') {
        pair.port2.postMessage({
          type: 'ready',
          requestId: message.requestId,
          clientId: message.clientId,
          documentId: message.documentId,
          role: message.role,
          sessionId: 'session',
          generation: `generation-${index}`,
          assets: [],
        } satisfies WebServerMessage);
      } else handler?.(message, pair.port2, index);
    });
    return { port: pair.port1 as unknown as PortLike };
  };
}
async function connect(handler?: Parameters<typeof factory>[0], extra = {}) {
  const client = await connectWebEditor({
    workerUrl: 'https://editor.test/worker.js',
    documentId: 'document',
    role: 'main',
    keyboard: false,
    createWorker: factory(handler),
    ...extra,
  });
  clients.push(client);
  return client;
}

beforeEach(() => {
  vi.stubGlobal(
    'URL',
    class extends URL {
      static createObjectURL = vi.fn(() => 'blob:fixture');
      static revokeObjectURL = vi.fn();
    },
  );
});
afterEach(() => {
  for (const client of clients.splice(0)) client.dispose();
  for (const port of ports.splice(0)) port.close();
  connections.length = 0;
  vi.unstubAllGlobals();
});

describe('SharedWorker client의 실제 MessagePort 계약', () => {
  it('준비된 host 역할과 실제 Tauri invoke 오류 구조를 유지한다', async () => {
    const error = {
      errorCode: 'IO_ERROR',
      retryable: true,
      message: 'storage unavailable',
    };
    await connect((message, port) => {
      if (message.type === 'invoke')
        port.postMessage({
          type: 'response',
          requestId: message.requestId,
          ok: false,
          error,
        });
    });
    expect(window.__dmn_window_type).toBe('main');
    expect(window.__dmn_runtime).toBe('web');
    await expect(
      invoke('editor_commit', { request: { mutationId: 'same' } }),
    ).rejects.toEqual(error);
  });

  it('emit_to를 실제 포트로 라우팅하고 이벤트 후 응답을 처리한다', async () => {
    const received = notify<WebClientMessage>();
    await connect((message, port) => {
      if (message.type !== 'emit') return;
      received.resolve(message);
      port.postMessage({
        type: 'event',
        event: message.event,
        payload: message.payload,
      });
      port.postMessage({
        type: 'response',
        requestId: message.requestId,
        ok: true,
        result: null,
      });
    });
    const events: unknown[] = [];
    const unlisten = await listen('sample', ({ payload }) =>
      events.push(payload),
    );
    await emitTo('overlay', 'sample', { value: 1 });
    expect(await received.promise).toMatchObject({
      type: 'emit',
      target: { kind: 'AnyLabel', label: 'overlay' },
    });
    expect(events).toEqual([{ value: 1 }]);
    await unlisten();
  });

  it('응답 유실 후 reconnect에서 동일 requestId를 재전송한다', async () => {
    const firstRequest =
      notify<Extract<WebClientMessage, { type: 'invoke' }>>();
    const replay = notify<Extract<WebClientMessage, { type: 'invoke' }>>();
    const client = await connect((message, port, index) => {
      if (message.type !== 'invoke') return;
      if (index === 0) firstRequest.resolve(message);
      else {
        replay.resolve(message);
        port.postMessage({
          type: 'response',
          requestId: message.requestId,
          ok: true,
          result: { revision: 7 },
        });
      }
    });
    const pending = client.invoke('editor_commit', {
      request: { mutationId: 'stable' },
    });
    const initial = await firstRequest.promise;
    await client.reconnect();
    expect(await replay.promise).toEqual(initial);
    await expect(pending).resolves.toEqual({ revision: 7 });
  });

  it('기존 Channel을 다시 구독하고 새 worker index를 이어서 전달한다', async () => {
    const first = notify<string>();
    const second = notify<string>();
    let delivered = 0;
    const client = await connect((message, port, index) => {
      if (message.type !== 'invoke') return;
      port.postMessage({
        type: 'response',
        requestId: message.requestId,
        ok: true,
        result: index + 1,
      });
      if (message.command === 'editor_preview_subscribe') {
        const callbackId = Number(String(message.args.channel).split(':')[1]);
        port.postMessage({
          type: 'callback',
          callbackId,
          payload: { index: 0, message: `preview-${index}` },
        });
      }
    });
    const channel = new Channel<string>((message) => {
      delivered += 1;
      if (delivered === 1) first.resolve(message);
      else second.resolve(message);
    });
    await invoke('editor_preview_subscribe', { channel });
    expect(await first.promise).toBe('preview-0');
    await client.reconnect();
    expect(await second.promise).toBe('preview-1');
  });

  it('자산을 이벤트보다 먼저 등록하고 다운로드를 완료한 뒤 응답한다', async () => {
    const download = vi.fn();
    const client = await connect(
      (message, port) => {
        if (message.type !== 'invoke') return;
        port.postMessage({
          type: 'assets',
          assets: [
            { key: '/assets/image', dataBase64: 'aGk=', mimeType: 'image/png' },
          ],
        });
        port.postMessage({
          type: 'event',
          event: 'asset-ready',
          payload: '/assets/image',
        });
        port.postMessage({
          type: 'response',
          requestId: message.requestId,
          ok: true,
          result: { success: true },
          download: {
            name: 'preset.json',
            mimeType: 'application/json',
            bytes: new ArrayBuffer(0),
          },
        });
      },
      { download },
    );
    let observed: string | undefined;
    const stop = await listen<string>('asset-ready', ({ payload }) => {
      observed = client.assets.resolve(payload);
    });
    await client.invoke('preset_save');
    expect(observed).toBe('blob:fixture');
    expect(download).toHaveBeenCalledOnce();
    await stop();
  });

  it('restores successful raw input subscriptions before replaying a lost unsubscribe', async () => {
    const counts = new Map<number, number>();
    const lost = notify<void>();
    const client = await connect((message, port, index) => {
      if (message.type !== 'invoke') return;
      const count = Math.max(
        0,
        (counts.get(index) ?? 0) +
          (message.command === 'raw_input_subscribe' ? 1 : -1),
      );
      counts.set(index, count);
      if (index === 0 && message.command === 'raw_input_unsubscribe') {
        lost.resolve();
        return;
      }
      port.postMessage({
        type: 'response',
        requestId: message.requestId,
        ok: true,
        result: { count },
      });
    });
    await client.invoke('raw_input_subscribe');
    await client.invoke('raw_input_subscribe');
    const pending = client.invoke('raw_input_unsubscribe');
    await lost.promise;
    await client.reconnect();
    await expect(pending).resolves.toEqual({ count: 1 });
    expect(counts.get(1)).toBe(1);
    expect(
      connections[1].messages
        .filter((message) => message.type === 'invoke')
        .map((message) => message.command),
    ).toEqual([
      'raw_input_subscribe',
      'raw_input_subscribe',
      'raw_input_unsubscribe',
    ]);
  });

  it('history flush acknowledgement reaches the worker while native quit stays unavailable', async () => {
    const client = await connect((message, port) => {
      if (message.type === 'invoke')
        port.postMessage({
          type: 'response',
          requestId: message.requestId,
          ok: true,
          result: null,
        });
    });
    await expect(
      client.invoke('app_quit_after_editor_flush', { handshakeId: 'history' }),
    ).resolves.toBeNull();
    expect(connections[0].messages).toContainEqual(
      expect.objectContaining({
        command: 'app_quit_after_editor_flush',
        args: { handshakeId: 'history' },
      }),
    );
    await expect(client.invoke('app_quit')).rejects.toMatchObject({
      code: 'UNSUPPORTED_WEB_COMMAND',
    });
  });

  it('pagehide detaches the role and bfcache pageshow reconnects the same client identity', async () => {
    const detached = notify<void>();
    const client = await connect((message) => {
      if (message.type === 'disconnect') detached.resolve();
    });
    const connected = notify<void>();
    const stop = client.subscribeConnectionState((state) => {
      if (state === 'connected') connected.resolve();
    });
    window.dispatchEvent(
      new PageTransitionEvent('pagehide', { persisted: true }),
    );
    await detached.promise;
    expect(client.connectionState).toBe('disconnected');
    window.dispatchEvent(
      new PageTransitionEvent('pageshow', { persisted: true }),
    );
    await connected.promise;
    expect(connections[1].messages[0]).toMatchObject({
      type: 'connect',
      clientId: client.clientId,
    });
    stop();
  });

  it('dispose rejects an in-progress reconnect immediately', async () => {
    const base = factory();
    let count = 0;
    const reconnectStarted = notify<void>();
    const createWorker = () => {
      if (++count === 1) return base();
      const pair = new MessageChannel();
      ports.push(pair.port1, pair.port2);
      pair.port2.on('message', () => reconnectStarted.resolve());
      return { port: pair.port1 as unknown as PortLike };
    };
    const client = await connect(undefined, { createWorker });
    const pending = client.reconnect();
    const rejected = expect(pending).rejects.toMatchObject({
      code: 'HOST_DISPOSED',
    });
    await reconnectStarted.promise;
    client.dispose();
    await rejected;
    expect(client.connectionState).toBe('disposed');
  });

  it('명시적 미지원 명령은 성공을 가장하지 않고 파일 취소는 기존 결과를 반환한다', async () => {
    const pickFiles = vi.fn().mockResolvedValue([]);
    const client = await connect(undefined, { pickFiles });
    await expect(client.invoke('window_minimize')).rejects.toMatchObject({
      code: 'UNSUPPORTED_WEB_COMMAND',
    });
    await expect(client.invoke('image_load')).resolves.toEqual({
      success: false,
    });
    expect(
      connections[0].messages.filter((message) => message.type === 'invoke'),
    ).toHaveLength(0);
  });
});
