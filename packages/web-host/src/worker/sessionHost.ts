import {
  WEB_PROTOCOL_VERSION,
  webHostError,
  type PortLike,
  type WebServerMessage,
} from '../protocol';
import type {
  EngineFactory,
  EditorEngine,
  KeyboardMatcher,
} from '../engine/wasm';
import { collectAssetKeys, type WebStorage } from '../storage/indexedDb';
import { createPreviewBroker } from './preview';
import { createFrontendFlush } from './frontendFlush';
import { isClientMessage } from './messageValidation';
import { createWebOverlayPreview } from './overlay';
import type { Client, Session } from './types';
import {
  assetsForClient,
  responseError,
  targetRole,
  send,
  broadcast,
} from './wire';
import { createEngineInvoker, readCommandReceipt } from './engineCommands';
import { createCommandRouter } from './commands';
import { restoreSessionEngine } from './engineRecovery';
import { dispatchInput, releaseClientInput } from './input';

export interface SessionHostOptions {
  storage: WebStorage;
  engines: EngineFactory;
  /** 실제 SharedWorker에서는 Web Locks로 다른 worker 버전의 중복 소유 방지 */
  acquireDocument?: (documentId: string) => Promise<() => void>;
  now?: () => number;
  randomId?: () => string;
}
export const createSessionHost = ({
  storage,
  engines,
  acquireDocument = async () => () => {},
  now = Date.now,
  randomId = () => crypto.randomUUID(),
}: SessionHostOptions) => {
  const generation = randomId();
  const sessions = new Map<string, Promise<Session>>();
  let disposed = false;
  const enqueue = <T>(
    session: Session,
    work: () => Promise<T> | T,
  ): Promise<T> => {
    const result = session.queue.then(work);
    session.queue = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  };
  const getSession = (documentId: string): Promise<Session> => {
    let pending = sessions.get(documentId);
    if (!pending) {
      pending = (async () => {
        const releaseLock = await acquireDocument(documentId);
        let engine: EditorEngine | undefined;
        let keyboard: KeyboardMatcher | undefined;
        try {
          const restored = await restoreSessionEngine(
            storage,
            engines,
            documentId,
            randomId,
          );
          engine = restored.engine;
          const store = restored.store;
          keyboard = engines.keyboard(
            store.keys,
            String(store.selectedKeyType),
          );
          const session: Session = {
            id: randomId(),
            documentId,
            incarnation: restored.incarnation,
            engine,
            keyboard,
            assets: restored.assets,
            assetModifiedAt: restored.assetModifiedAt,
            version: restored.version,
            clients: new Map(),
            queue: Promise.resolve(),
            releaseLock,
            closing: false,
            skipAssetSweep: engine.recovered ?? false,
            overlay: createWebOverlayPreview(),
            flush: createFrontendFlush((clientId, event, payload) => {
              const client = session.clients.get(clientId);
              if (client) send(client, { type: 'event', event, payload });
            }),
            preview: createPreviewBroker((clientId, callbackId, payload) => {
              const client = session.clients.get(clientId);
              if (client)
                send(client, { type: 'callback', callbackId, payload });
            }, engines.preview),
          };
          session.overlay.sync(
            engine.read('overlay_get', {}) as {
              visible: boolean;
              locked: boolean;
              anchor: string;
            },
          );
          return session;
        } catch (error) {
          keyboard?.dispose();
          engine?.dispose();
          releaseLock();
          sessions.delete(documentId);
          throw error;
        }
      })();
      sessions.set(documentId, pending);
    }
    return pending;
  };
  const invokeEngine = createEngineInvoker(storage, engines, now, randomId);
  const invoke = createCommandRouter(invokeEngine);
  const retireMainAuthority = async (session: Session, client: Client) => {
    if (client.role !== 'main') return;
    await invokeEngine(session, client, {
      type: 'invoke',
      requestId: randomId(),
      command: 'web_main_disconnected',
      args: {},
    });
  };
  const closeEmptySession = async (session: Session) => {
    if (session.clients.size || session.closing) return;
    session.closing = true;
    // 히스토리가 살아 있는 동안은 자산 격리·삭제를 실행하지 않는다.
    try {
      if (!session.skipAssetSweep)
        await storage.collectUnusedAssets(
          session.documentId,
          collectAssetKeys(session.engine.snapshot().store),
        );
    } finally {
      session.engine.dispose();
      session.keyboard.dispose();
      session.releaseLock();
      sessions.delete(session.documentId);
    }
  };
  const disconnect = async (session: Session, client: Client) => {
    if (session.clients.get(client.id) !== client) return;
    try {
      await retireMainAuthority(session, client);
    } finally {
      releaseClientInput(session, client);
      session.flush.disconnect(client.id);
      session.preview.disconnect(client.id);
      session.clients.delete(client.id);
      client.port.close();
      await closeEmptySession(session);
    }
  };
  return {
    attach(port: PortLike) {
      let client: Client | undefined;
      let session: Session | undefined;
      let attaching = false;
      const receive = (event: MessageEvent) => {
        const message: unknown = event.data;
        if (disposed) return;
        if (!isClientMessage(message)) {
          port.postMessage({
            type: 'error',
            error: webHostError(
              'INVALID_MESSAGE',
              'Malformed web host message',
            ),
          } satisfies WebServerMessage);
          return;
        }
        if (message.type === 'connect') {
          if (client || attaching) return;
          attaching = true;
          void (async () => {
            if (message.version !== WEB_PROTOCOL_VERSION)
              throw webHostError(
                'PROTOCOL_VERSION_MISMATCH',
                'Web host and client versions do not match',
              );
            if (
              !['main', 'overlay'].includes(message.role) ||
              !message.documentId?.trim() ||
              message.documentId.length > 200 ||
              !message.clientId?.trim() ||
              message.clientId.length > 200
            )
              throw webHostError(
                'INVALID_SESSION_IDENTITY',
                'Invalid document or client identity',
              );
            session = await getSession(message.documentId);
            await session.queue;
            if (session.closing) session = await getSession(message.documentId);
            await enqueue(session, async () => {
              const duplicate = [...session!.clients.values()].find(
                (existing) =>
                  existing.role === message.role &&
                  existing.id !== message.clientId,
              );
              if (duplicate)
                throw webHostError(
                  'ROLE_ALREADY_CONNECTED',
                  `This document already has a ${message.role} client`,
                );
              const previous = session!.clients.get(message.clientId);
              if (previous && previous.role !== message.role)
                throw webHostError(
                  'CLIENT_ROLE_MISMATCH',
                  'A client ID cannot change roles',
                );
              const sameRealm = previous?.realmId === message.realmId;
              const nextClient: Client = {
                id: message.clientId,
                realmId: message.realmId,
                role: message.role,
                port,
                rawSubscriptions: 0,
                rawReceipts: new Map(),
                keys: new Map(),
              };
              try {
                if (!sameRealm) await retireMainAuthority(session!, nextClient);
              } finally {
                if (previous) {
                  session!.flush.disconnect(previous.id);
                  session!.preview.disconnect(previous.id);
                  releaseClientInput(session!, previous);
                  session!.clients.delete(previous.id);
                  previous.port.close();
                }
              }
              client = nextClient;
              session!.clients.set(client.id, client);
              send(client, {
                type: 'ready',
                requestId: message.requestId,
                documentId: message.documentId,
                role: message.role,
                clientId: client.id,
                sessionId: session!.id,
                generation,
                pluginAuthorityResetRequired:
                  client.role === 'main' &&
                  (!sameRealm ||
                    session!.engine.snapshot().checkpoint.authorityAvailable ===
                      false),
                assets: assetsForClient(session!.assets),
              });
              send(client, {
                type: 'event',
                event: 'web:overlay-state',
                payload: session!.overlay.snapshot(),
              });
            });
          })()
            .catch(async (error) => {
              if (session)
                await enqueue(session, () => closeEmptySession(session!)).catch(
                  (cleanupError) => {
                    console.warn(
                      'Failed to collect assets after rejected attachment',
                      cleanupError,
                    );
                  },
                );
              port.postMessage({
                type: 'error',
                error: responseError(error),
              } satisfies WebServerMessage);
              port.close();
            })
            .finally(() => {
              attaching = false;
            });
          return;
        }
        if (!client || !session || session.clients.get(client.id) !== client)
          return;
        const currentClient = client;
        const currentSession = session;
        if (
          message.type === 'invoke' &&
          (message.command === 'history_undo' ||
            message.command === 'history_redo')
        ) {
          void (async () => {
            const receipt = await readCommandReceipt(
              storage,
              currentSession,
              message,
            );
            if (receipt) {
              send(currentClient, {
                type: 'response',
                requestId: message.requestId,
                ok: true,
                result: currentSession.engine.read('history_status', {}),
              });
              return;
            }
            const flush = await currentSession.flush.begin(
              currentSession.clients.keys(),
            );
            try {
              await enqueue(currentSession, async () => {
                const response = await invoke(
                  currentSession,
                  currentClient,
                  message,
                );
                send(currentClient, {
                  type: 'response',
                  requestId: message.requestId,
                  ok: true,
                  ...response,
                });
              });
            } finally {
              flush.complete();
            }
          })().catch((error) =>
            send(currentClient, {
              type: 'response',
              requestId: message.requestId,
              ok: false,
              error: responseError(error),
            }),
          );
          return;
        }
        // admission은 큐 진입 시점에 검사해 이미 접수된 저장이 flush ack 뒤 거절되지 않게 한다.
        if (
          message.type === 'invoke' &&
          (engines.kind(message.command) === 'write' ||
            message.command === 'editor_preview_subscribe' ||
            message.command === 'editor_preview_publish')
        ) {
          try {
            currentSession.flush.assertMutationAllowed(currentClient.id);
          } catch (error) {
            send(currentClient, {
              type: 'response',
              requestId: message.requestId,
              ok: false,
              error: responseError(error),
            });
            return;
          }
        }
        void enqueue(currentSession, async () => {
          if (currentSession.clients.get(currentClient.id) !== currentClient)
            return;
          if (message.type === 'invoke') {
            try {
              const response = await invoke(
                currentSession,
                currentClient,
                message,
              );
              send(currentClient, {
                type: 'response',
                requestId: message.requestId,
                ok: true,
                ...response,
              });
            } catch (error) {
              send(currentClient, {
                type: 'response',
                requestId: message.requestId,
                ok: false,
                error: responseError(error),
              });
            }
          } else if (message.type === 'emit') {
            try {
              broadcast(
                currentSession,
                {
                  type: 'event',
                  event: message.event,
                  payload: message.payload,
                },
                targetRole(message.target),
              );
              send(currentClient, {
                type: 'response',
                requestId: message.requestId,
                ok: true,
                result: null,
              });
            } catch (error) {
              send(currentClient, {
                type: 'response',
                requestId: message.requestId,
                ok: false,
                error: responseError(error),
              });
            }
          } else if (message.type === 'input')
            await dispatchInput(
              currentSession,
              currentClient,
              message,
              invokeEngine,
              randomId,
            );
          else if (message.type === 'disconnect')
            await disconnect(currentSession, currentClient);
        }).catch((error) =>
          send(currentClient, {
            type: 'event',
            event: 'web:error',
            payload: responseError(error),
          }),
        );
      };
      port.addEventListener('message', receive);
      port.start();
      return () => {
        port.removeEventListener('message', receive);
        if (session && client)
          void enqueue(session, () => disconnect(session!, client!));
      };
    },
    async dispose() {
      disposed = true;
      for (const pending of sessions.values()) {
        const session = await pending;
        await session.queue;
        for (const client of session.clients.values()) client.port.close();
        session.engine.dispose();
        session.keyboard.dispose();
        session.releaseLock();
      }
      sessions.clear();
      storage.close();
    },
  };
};
