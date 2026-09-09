import { webHostError } from '../protocol';
import type { EngineFactory } from '../engine/wasm';
import { StorageConflictError, type WebStorage } from '../storage/indexedDb';
import type { Client, Session, Invoke } from './types';
import { restoreSessionEngine } from './engineRecovery';
import {
  assetsForClient,
  encodeBytes,
  fingerprint,
  ioError,
  broadcast,
} from './wire';

/** receipt 조회 오류도 편집 임시 실패로 보존, 복구 전 receipt는 재적용하지 않음 */
export const readCommandReceipt = async (
  storage: WebStorage,
  session: Session,
  message: Invoke,
  digest?: string,
) => {
  let receipt;
  try {
    receipt = await storage.receipt(session.documentId, message.requestId);
  } catch (error) {
    throw ioError(error);
  }
  if (!receipt) return undefined;
  if ((receipt.incarnation ?? 'original') !== session.incarnation) {
    const snapshot = session.engine.read('editor_get', {}) as {
      revision: number;
    };
    throw {
      errorCode: 'REVISION_CONFLICT',
      message: 'editor revision conflict',
      details: { currentRevision: snapshot.revision },
      retryable: true,
    };
  }
  const expected =
    digest ?? (await fingerprint(message.command, message.args, message.files));
  if (receipt.fingerprint !== expected)
    throw webHostError(
      'REQUEST_ID_REUSED',
      'A request ID was reused for a different mutation',
    );
  return receipt;
};

/** Rust 준비 → IndexedDB 저장 → 확정 → 이벤트 발행의 단일 실행 경계 */
export const createEngineInvoker = (
  storage: WebStorage,
  engines: EngineFactory,
  now: () => number,
  randomId: () => string = () => crypto.randomUUID(),
) => {
  const reloadAfterConflict = async (session: Session) => {
    const restored = await restoreSessionEngine(
      storage,
      engines,
      session.documentId,
      randomId,
    );
    session.engine.dispose();
    session.engine = restored.engine;
    session.skipAssetSweep ||= session.engine.recovered ?? false;
    session.version = restored.version;
    session.incarnation = restored.incarnation;
    session.assets = restored.assets;
    session.assetModifiedAt = restored.assetModifiedAt;
    const store = restored.store;
    session.keyboard.clear();
    session.keyboard.update(store.keys, String(store.selectedKeyType));
    broadcast(session, {
      type: 'assets',
      assets: assetsForClient(session.assets),
    });
    broadcast(session, { type: 'event', event: 'obs:resync', payload: null });
  };
  const invokeEngine = async (
    session: Session,
    client: Client,
    message: Invoke,
  ): Promise<unknown> => {
    const { command, requestId } = message;
    const kind = engines.kind(command);
    if (client.role !== 'main') {
      if (command === 'plugin_authority_reset')
        throw 'PLUGIN_AUTHORITY_RESET_NOT_ALLOWED';
      if (
        command === 'plugin_instances_commit' ||
        command === 'plugin_instances_reconcile'
      )
        throw 'PLUGIN_INSTANCE_MUTATION_NOT_ALLOWED';
      if (command === 'commit_gesture') throw 'GESTURE_COMMIT_NOT_ALLOWED';
    }
    if (kind === 'unsupported')
      throw webHostError(
        'UNSUPPORTED_WEB_COMMAND',
        `Command is not available in this browser host: ${command}`,
      );
    const args = {
      ...message.args,
      ...(/^(preset_|sound_|css_|js_)/.test(command) ||
      command === 'image_load' ||
      command === 'font_load' ||
      command === 'keys_reset_all' ||
      command === 'keys_reset_mode'
        ? {
            assets: session.assets,
            assetModifiedAt: session.assetModifiedAt,
            availablePaths: Object.keys(session.assets),
          }
        : {}),
      timestampMs: now(),
      sourceLabel: client.role,
      ...(message.files
        ? {
            files: message.files.map((file) => ({
              name: file.name,
              mimeType: file.mimeType,
              dataBase64: encodeBytes(file.bytes),
            })),
          }
        : {}),
    };
    if (kind === 'read') {
      const result = session.engine.read(command, args);
      if (command === 'app_bootstrap' && result && typeof result === 'object')
        Object.assign(result, { activeKeys: session.keyboard.active().keys });
      return result;
    }
    const durableReceipt =
      command !== 'web_input_press' && command !== 'web_main_disconnected';
    const digest = durableReceipt
      ? await fingerprint(command, message.args, message.files)
      : '';
    const receipt = durableReceipt
      ? await readCommandReceipt(storage, session, message, digest)
      : undefined;
    if (receipt) return receipt.result;
    const beforeStore = session.engine.snapshot().store;
    const prepared = session.engine.prepare(command, args, requestId);
    try {
      session.version = await storage.save({
        documentId: session.documentId,
        expectedVersion: session.version,
        incarnation: session.incarnation,
        store: prepared.store ?? session.engine.snapshot().store,
        checkpoint: prepared.checkpoint,
        assetWrites: prepared.assetWrites,
        assetDeletes: prepared.assetDeletes,
        receipt: durableReceipt
          ? {
              documentId: session.documentId,
              requestId,
              fingerprint: digest,
              incarnation: session.incarnation,
              result: prepared.result,
              committedAt: now(),
            }
          : undefined,
      });
    } catch (error) {
      session.engine.discard(requestId);
      if (error instanceof StorageConflictError) {
        try {
          await reloadAfterConflict(session);
        } catch (reloadError) {
          throw ioError(reloadError);
        }
      }
      throw ioError(error);
    }
    const confirmed = session.engine.confirm(requestId);
    const request = message.args.request as Record<string, unknown> | undefined;
    if (command === 'editor_commit' || command === 'commit_gesture') {
      const gestureIds = Array.isArray(request?.gestureIds)
        ? request.gestureIds
        : [request?.gestureId];
      for (const gestureId of gestureIds)
        if (typeof gestureId === 'string')
          try {
            session.preview.finishCommittedSession(
              client.id,
              client.role,
              gestureId,
              !confirmed.events.some(
                (event) => event.event === 'editor:committed',
              ),
            );
          } catch (error) {
            console.warn('Failed to finish committed preview', error);
          }
    }
    if (command === 'history_undo' || command === 'history_redo')
      session.preview.cancelAll();
    const removed = prepared.assetDeletes ?? [];
    for (const key of removed) {
      delete session.assets[key];
      delete session.assetModifiedAt[key];
    }
    if (removed.length)
      broadcast(session, { type: 'assets', assets: [], removed });
    const writes = prepared.assetWrites ?? {};
    if (Object.keys(writes).length) {
      Object.assign(session.assets, writes);
      for (const key of Object.keys(writes))
        session.assetModifiedAt[key] = now();
      broadcast(session, { type: 'assets', assets: assetsForClient(writes) });
    }
    const store = session.engine.snapshot().store;
    if (
      beforeStore.selectedKeyType !== store.selectedKeyType ||
      JSON.stringify(beforeStore.keys) !== JSON.stringify(store.keys)
    )
      session.keyboard.update(store.keys, String(store.selectedKeyType));
    for (const event of confirmed.events)
      broadcast(session, { type: 'event', ...event });
    if (
      confirmed.events.some(
        (event) =>
          event.event.startsWith('overlay:') ||
          event.event === 'settings:changed',
      )
    ) {
      session.overlay.sync(
        session.engine.read('overlay_get', {}) as {
          visible: boolean;
          locked: boolean;
          anchor: string;
        },
      );
      broadcast(session, {
        type: 'event',
        event: 'web:overlay-state',
        payload: session.overlay.snapshot(),
      });
    }
    return confirmed.result;
  };
  return invokeEngine;
};
