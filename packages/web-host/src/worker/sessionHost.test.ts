import { MessageChannel, type MessagePort } from 'node:worker_threads';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createSessionHost } from './sessionHost';
import type {
  EditorEngine,
  EngineFactory,
  EngineSnapshot,
  PreparedCommand,
} from '../engine/wasm';
import type {
  SaveWorkspace,
  StoredDocument,
  StoredReceipt,
  WebStorage,
} from '../storage/indexedDb';
import type { PortLike, WebClientMessage, WebServerMessage } from '../protocol';
import type { PreviewEnvelope } from './previewValidation';

const hosts: ReturnType<typeof createSessionHost>[] = [];
const ports: MessagePort[] = [];
afterEach(async () => {
  for (const host of hosts.splice(0)) await host.dispose();
  for (const port of ports.splice(0)) port.close();
});
function fixture() {
  let document: StoredDocument | undefined;
  const receipts = new Map<string, StoredReceipt>();
  const save = vi.fn(async (change: SaveWorkspace) => {
    document = {
      id: change.documentId,
      version: change.expectedVersion + 1,
      store: structuredClone(change.store),
    };
    if (change.receipt)
      receipts.set(change.receipt.requestId, structuredClone(change.receipt));
    return document.version;
  });
  const storage: WebStorage = {
    load: async () => ({
      document,
      assets: {},
      assetModifiedAt: {},
      quarantinedKeys: [],
    }),
    receipt: async (_doc, id) => receipts.get(id),
    save,
    collectUnusedAssets: async () => {},
    close() {},
  };
  const confirms = vi.fn();
  const engineDispose = vi.fn();
  const keyboardDispose = vi.fn();
  const preparedRequests = vi.fn();
  const engines: EngineFactory = {
    create(initial) {
      let value = Number(initial?.value ?? 0);
      let previous = 0;
      const pending = new Map<string, PreparedCommand>();
      const snapshot = (): EngineSnapshot => ({
        store: { value, keys: {}, selectedKeyType: '4key' },
        history: {},
        pluginModelRevision: 0,
        authorityGeneration: 0,
        checkpoint: {},
      });
      return {
        snapshot,
        read: () => snapshot().store,
        prepare(command, args, token) {
          preparedRequests(command, args);
          if (command === 'web_main_disconnected') {
            const prepared: PreparedCommand = {
              token,
              store: null,
              checkpoint: { authorityAvailable: false },
              result: null,
              events: [],
            };
            pending.set(token, prepared);
            return prepared;
          }
          const next =
            command === 'history_undo'
              ? previous
              : Number((args.request as { value: number }).value);
          const prepared: PreparedCommand = {
            token,
            checkpoint: {},
            store: { ...snapshot().store, value: next },
            result: { value: next },
            events: [{ event: 'editor:committed', payload: { value: next } }],
          };
          pending.set(token, prepared);
          return prepared;
        },
        confirm(token) {
          const prepared = pending.get(token)!;
          if (prepared.store === null) {
            pending.delete(token);
            return prepared;
          }
          confirms(token);
          pending.delete(token);
          previous = value;
          value = Number(prepared.store!.value);
          return prepared;
        },
        discard: (token) => {
          pending.delete(token);
        },
        dispose: engineDispose,
      } satisfies EditorEngine;
    },
    kind: (command) =>
      ['editor_commit', 'history_undo', 'web_main_disconnected'].includes(
        command,
      )
        ? 'write'
        : command === 'editor_get'
        ? 'read'
        : 'unsupported',
    keyboard: () => ({
      update() {},
      feed: () => null,
      active: () => ({ mode: '4key', keys: [] }),
      clear() {},
      dispose: keyboardDispose,
    }),
    preview: {
      validatePublish: (value, sourceLabel) => ({
        ...(value as PreviewEnvelope),
        sourceLabel,
      }),
      isSessionId: () => true,
    },
  };
  const host = createSessionHost({ storage, engines });
  hosts.push(host);
  let sequence = 0;
  async function connect(
    clientId: string,
    role: 'main' | 'overlay',
    realmId = clientId,
  ) {
    const pair = new MessageChannel();
    ports.push(pair.port1, pair.port2);
    host.attach(pair.port2 as unknown as PortLike);
    const messages: WebServerMessage[] = [];
    const waiters: Array<{
      predicate: (message: WebServerMessage) => boolean;
      resolve: (message: WebServerMessage) => void;
    }> = [];
    pair.port1.on('message', (message: WebServerMessage) => {
      messages.push(message);
      for (const waiter of [...waiters])
        if (waiter.predicate(message)) {
          waiters.splice(waiters.indexOf(waiter), 1);
          waiter.resolve(message);
        }
    });
    const next = (predicate: (message: WebServerMessage) => boolean) =>
      new Promise<WebServerMessage>((resolve) => {
        waiters.push({ predicate, resolve });
      });
    const send = (message: WebClientMessage) => pair.port1.postMessage(message);
    const ready = next(
      (message) => message.type === 'ready' || message.type === 'error',
    );
    send({
      type: 'connect',
      version: 1,
      requestId: `connect-${++sequence}`,
      clientId,
      realmId,
      role,
      documentId: 'doc',
    });
    const readyMessage = await ready;
    const invoke = (
      command: string,
      args: Record<string, unknown> = {},
      requestId = `request-${++sequence}`,
    ) => {
      const response = next(
        (message) =>
          message.type === 'response' && message.requestId === requestId,
      );
      send({ type: 'invoke', command, args, requestId });
      return response;
    };
    return { send, next, invoke, messages, ready: readyMessage };
  }
  return {
    connect,
    save,
    confirms,
    storage,
    engineDispose,
    keyboardDispose,
    preparedRequests,
  };
}

describe('SharedWorker session host ordering', () => {
  it('publishes after durable save, does not confirm failures, and replays a lost mutation once', async () => {
    const { connect, save, confirms } = fixture();
    const main = await connect('a', 'main');
    const overlay = await connect('b', 'overlay');
    save.mockRejectedValueOnce(new Error('disk full'));
    expect(
      await main.invoke('editor_commit', { request: { value: 1 } }),
    ).toMatchObject({
      ok: false,
      error: { errorCode: 'IO_ERROR', retryable: true },
    });
    expect(confirms).not.toHaveBeenCalled();
    expect(
      overlay.messages.filter(
        (message) =>
          message.type === 'event' && message.event === 'editor:committed',
      ),
    ).toEqual([]);
    const changed = overlay.next(
      (message) =>
        message.type === 'event' && message.event === 'editor:committed',
    );
    const args = { request: { value: 2 } };
    expect(
      await main.invoke('editor_commit', args, 'lost-response'),
    ).toMatchObject({ ok: true, result: { value: 2 } });
    await changed;
    const replacement = await connect('a', 'main', 'new-page');
    expect(save.mock.calls.at(-1)?.[0].checkpoint).toEqual({
      authorityAvailable: false,
    });
    expect(await replacement.invoke('web_main_disconnected')).toMatchObject({
      ok: false,
      error: { code: 'UNSUPPORTED_WEB_COMMAND' },
    });
    expect(
      await replacement.invoke('editor_commit', args, 'lost-response'),
    ).toMatchObject({ ok: true, result: { value: 2 } });
    expect(confirms).toHaveBeenCalledTimes(1);
    expect(
      await replacement.invoke(
        'editor_commit',
        { request: { value: 3 } },
        'lost-response',
      ),
    ).toMatchObject({ ok: false, error: { code: 'REQUEST_ID_REUSED' } });
  });
  it('preserves same-realm authority and requires reset for new realms, including retirement failure cleanup', async () => {
    const { connect, save, preparedRequests } = fixture();
    const first = await connect('a', 'main', 'realm-one');
    expect(first.ready).toMatchObject({ pluginAuthorityResetRequired: true });
    const retirementCount = () =>
      preparedRequests.mock.calls.filter(
        ([command]) => command === 'web_main_disconnected',
      ).length;
    const count = retirementCount();
    const same = await connect('a', 'main', 'realm-one');
    expect(same.ready).toMatchObject({ pluginAuthorityResetRequired: false });
    expect(retirementCount()).toBe(count);
    save.mockRejectedValueOnce(new Error('quota'));
    const failed = await connect('a', 'main', 'realm-two');
    expect(failed.ready).toMatchObject({
      type: 'error',
      error: { errorCode: 'IO_ERROR' },
    });
    const replacement = await connect('new-client', 'main', 'realm-three');
    expect(replacement.ready).toMatchObject({
      type: 'ready',
      pluginAuthorityResetRequired: true,
    });
  });
  it('keeps receipt read failures retryable and frees partially initialized WASM handles', async () => {
    const first = fixture();
    first.save.mockRejectedValueOnce(new Error('quota'));
    expect((await first.connect('a', 'main')).ready).toMatchObject({
      type: 'error',
    });
    expect(first.engineDispose).toHaveBeenCalledOnce();
    expect(first.keyboardDispose).not.toHaveBeenCalled();
    const next = fixture();
    const main = await next.connect('a', 'main');
    vi.spyOn(next.storage, 'receipt').mockRejectedValueOnce(
      new Error('IndexedDB unavailable'),
    );
    expect(
      await main.invoke('editor_commit', { request: { value: 3 } }),
    ).toMatchObject({
      ok: false,
      error: { errorCode: 'IO_ERROR', retryable: true },
    });
    expect(next.confirms).not.toHaveBeenCalled();
  });

  it('releases an empty session after failed initial main retirement', async () => {
    const { connect, save, engineDispose, keyboardDispose } = fixture();
    const implementation = save.getMockImplementation()!;
    save
      .mockImplementationOnce(implementation)
      .mockRejectedValueOnce(new Error('retirement quota'));
    expect((await connect('a', 'main')).ready).toMatchObject({
      type: 'error',
      error: { errorCode: 'IO_ERROR' },
    });
    expect(engineDispose).toHaveBeenCalledOnce();
    expect(keyboardDispose).toHaveBeenCalledOnce();
    expect((await connect('new', 'main')).ready).toMatchObject({
      type: 'ready',
    });
  });
  it('allows in-flight edits during history flush and waits for both window acknowledgements', async () => {
    const { connect } = fixture();
    const main = await connect('a', 'main');
    const overlay = await connect('b', 'overlay');
    await main.invoke('editor_commit', { request: { value: 1 } });
    await main.invoke('editor_preview_subscribe', { channel: '__CHANNEL__:1' });
    const flushMain = main.next(
      (message) =>
        message.type === 'event' && message.event === 'app:close-requested',
    );
    const flushOverlay = overlay.next(
      (message) =>
        message.type === 'event' && message.event === 'app:close-requested',
    );
    const undo = main.invoke('history_undo', {
      request: { operationId: 'undo' },
    });
    const [first] = await Promise.all([flushMain, flushOverlay]);
    if (first.type !== 'event') throw new Error('expected flush event');
    const handshakeId = (first.payload as { handshakeId: string }).handshakeId;
    expect(
      await main.invoke('editor_commit', { request: { value: 2 } }),
    ).toMatchObject({ ok: true });
    await main.invoke('app_quit_after_editor_flush', { handshakeId });
    for (const [command, args] of [
      ['editor_preview_subscribe', { channel: '__CHANNEL__:2' }],
      [
        'editor_preview_publish',
        { request: { sessionId: crypto.randomUUID(), seq: 1 } },
      ],
    ] as const)
      expect(await main.invoke(command, args)).toMatchObject({
        ok: false,
        error: { errorCode: 'HISTORY_IN_PROGRESS' },
      });
    expect(
      await main.invoke('editor_commit', { request: { value: 3 } }),
    ).toMatchObject({ ok: false, error: { errorCode: 'HISTORY_IN_PROGRESS' } });
    expect(
      await overlay.invoke('editor_commit', { request: { value: 4 } }),
    ).toMatchObject({ ok: true });
    const released = overlay.next(
      (message) =>
        message.type === 'event' &&
        message.event === 'app:history-flush-released',
    );
    await overlay.invoke('app_quit_after_editor_flush', { handshakeId });
    expect(await undo).toMatchObject({ ok: true, result: { value: 2 } });
    await released;
    expect(
      await main.invoke('editor_preview_subscribe', {
        channel: '__CHANNEL__:3',
      }),
    ).toMatchObject({ ok: true });
    expect(
      await main.invoke('editor_commit', { request: { value: 5 } }),
    ).toMatchObject({ ok: true });
  });
  it('routes target events by role and preserves preview Channel callback order', async () => {
    const { connect } = fixture();
    const main = await connect('a', 'main');
    const overlay = await connect('b', 'overlay');
    await main.invoke('editor_preview_subscribe', { channel: '__CHANNEL__:1' });
    await overlay.invoke('editor_preview_subscribe', {
      channel: '__CHANNEL__:2',
    });
    const callback = overlay.next((message) => message.type === 'callback');
    await main.invoke('editor_preview_publish', {
      request: {
        schemaVersion: 1,
        sessionId: crypto.randomUUID(),
        seq: 1,
        kind: 'patch',
        domain: 'keyPosition',
        mode: '4key',
        targets: [0],
        patch: { dx: 2 },
      },
    });
    expect(await callback).toMatchObject({
      type: 'callback',
      callbackId: 2,
      payload: { index: 0, message: { sourceLabel: 'main' } },
    });
    expect(main.messages.some((message) => message.type === 'callback')).toBe(
      false,
    );
    const event = overlay.next(
      (message) => message.type === 'event' && message.event === 'targeted',
    );
    main.send({
      type: 'emit',
      requestId: 'emit',
      target: { kind: 'AnyLabel', label: 'overlay' },
      event: 'targeted',
      payload: 1,
    });
    await event;
    expect(
      main.messages.some(
        (message) => message.type === 'event' && message.event === 'targeted',
      ),
    ).toBe(false);
  });
});
