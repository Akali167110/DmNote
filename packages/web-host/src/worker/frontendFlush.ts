import { webHostError } from '../protocol';

export type FrontendFlushSend = (
  clientId: string,
  event: string,
  payload: { handshakeId: string; action?: 'history' },
) => void;
export interface FrontendFlushReady {
  handshakeId: string;
  complete(): void;
}
interface Handshake {
  id: string;
  targets: Set<string>;
  pending: Set<string>;
  running: boolean;
  timeout: ReturnType<typeof setTimeout>;
  resolve: (ready: FrontendFlushReady) => void;
  reject: (error: unknown) => void;
}

/** 큐 밖 flush 수집 후 호출자가 큐 barrier에서 history 적용 */
export function createFrontendFlush(send: FrontendFlushSend) {
  let active: Handshake | null = null;
  const release = (handshake: Handshake) => {
    if (active !== handshake) return;
    active = null;
    clearTimeout(handshake.timeout);
    for (const clientId of handshake.targets) {
      try {
        send(clientId, 'app:history-flush-released', {
          handshakeId: handshake.id,
        });
      } catch (error) {
        console.warn('History flush release delivery failed', error);
      }
    }
  };
  const fail = (handshake: Handshake, code: string) => {
    if (active !== handshake || handshake.running) return;
    release(handshake);
    handshake.reject(webHostError(code, code, true));
  };
  const ready = (handshake: Handshake) => {
    if (handshake.pending.size || active !== handshake || handshake.running)
      return;
    handshake.running = true;
    clearTimeout(handshake.timeout);
    handshake.resolve({
      handshakeId: handshake.id,
      complete: () => release(handshake),
    });
  };
  const owned = (clientId: string, handshakeId: unknown): Handshake => {
    if (!active || active.id !== handshakeId || !active.targets.has(clientId))
      throw webHostError(
        'INVALID_HISTORY_HANDSHAKE',
        'History flush handshake is not active for this client',
      );
    return active;
  };
  return {
    begin(clientIds: Iterable<string>): Promise<FrontendFlushReady> {
      if (active)
        return Promise.reject(
          webHostError(
            'HISTORY_FRONTEND_FLUSH_BUSY',
            'History flush is already active',
            true,
          ),
        );
      return new Promise((resolve, reject) => {
        const handshake: Handshake = {
          id: crypto.randomUUID(),
          targets: new Set(clientIds),
          pending: new Set(),
          running: false,
          timeout: setTimeout(
            () => fail(handshake, 'HISTORY_FRONTEND_FLUSH_TIMEOUT'),
            10000,
          ),
          resolve,
          reject,
        };
        handshake.pending = new Set(handshake.targets);
        active = handshake;
        try {
          for (const clientId of handshake.targets)
            send(clientId, 'app:close-requested', {
              handshakeId: handshake.id,
              action: 'history',
            });
        } catch {
          fail(handshake, 'HISTORY_FRONTEND_FLUSH_EMIT_FAILED');
          return;
        }
        ready(handshake);
      });
    },
    acknowledge(clientId: string, handshakeId: unknown): void {
      const handshake = owned(clientId, handshakeId);
      handshake.pending.delete(clientId);
      ready(handshake);
    },
    cancel(clientId: string, handshakeId: unknown): void {
      const handshake = owned(clientId, handshakeId);
      if (handshake.running)
        throw webHostError(
          'HISTORY_FRONTEND_FLUSH_BUSY',
          'History operation has already started',
          true,
        );
      fail(handshake, 'HISTORY_FRONTEND_FLUSH_CANCELED');
    },
    disconnect(clientId: string): void {
      if (active?.targets.has(clientId))
        fail(active, 'HISTORY_FRONTEND_FLUSH_INTERRUPTED');
    },
    assertMutationAllowed(clientId: string): void {
      if (active && !active.pending.has(clientId))
        throw {
          errorCode: 'HISTORY_IN_PROGRESS',
          message: 'history operation is in progress',
          retryable: true,
        };
    },
  };
}
