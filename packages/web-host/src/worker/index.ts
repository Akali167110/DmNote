import { loadWasmEngine } from '../engine/wasm';
import { openWebStorage } from '../storage/indexedDb';
import { createSessionHost } from './sessionHost';
import { webHostError, type PortLike } from '../protocol';

const databaseName = 'dmnote-web-editor-v1';
const acquireDocument = (documentId: string): Promise<() => void> =>
  new Promise((resolve, reject) => {
    if (!navigator.locks) {
      reject(
        webHostError(
          'WEB_LOCKS_UNAVAILABLE',
          'This browser requires Web Locks in a secure context',
        ),
      );
      return;
    }
    void navigator.locks
      .request(
        `${databaseName}:${documentId}`,
        { ifAvailable: true },
        async (lock) => {
          if (!lock) {
            reject(
              webHostError(
                'DOCUMENT_IN_USE',
                'Another web host version owns this document. Close its editor before reconnecting.',
                true,
              ),
            );
            return;
          }
          await new Promise<void>((release) => resolve(release));
        },
      )
      .catch(reject);
  });
const ready = Promise.all([
  openWebStorage({ name: databaseName }),
  loadWasmEngine(),
]).then(([storage, engines]) =>
  createSessionHost({ storage, engines, acquireDocument }),
);
const scope = globalThis as typeof globalThis & {
  onconnect: ((event: MessageEvent & { ports: PortLike[] }) => void) | null;
};
scope.onconnect = (event) => {
  for (const port of event.ports)
    void ready
      .then((host) => host.attach(port))
      .catch((error) =>
        port.postMessage({
          type: 'error',
          error: webHostError(
            'HOST_INITIALIZATION_FAILED',
            String(error),
            true,
          ),
        }),
      );
};
