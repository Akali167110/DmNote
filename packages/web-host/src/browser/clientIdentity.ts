import type { WebRole } from '../protocol';
import { webHostError } from '../protocol';

interface ClientIdentity {
  clientId: string;
  release(): void;
  reacquire(): Promise<void>;
}
const takeLease = (
  locks: LockManager,
  clientId: string,
): Promise<(() => void) | null> =>
  new Promise((resolve, reject) => {
    void locks
      .request(
        `dmnote:web:client:${clientId}`,
        { ifAvailable: true },
        (lock) => {
          if (!lock) {
            resolve(null);
            return;
          }
          return new Promise<void>((release) => resolve(release));
        },
      )
      .catch(reject);
  });

/** reload는 같은 ID, 복제 탭은 별도 ID. 닫힌 포트의 역할 등록 대체에 사용 */
export async function createBrowserClientIdentity(
  documentId: string,
  role: WebRole,
  workerName: string,
): Promise<ClientIdentity> {
  const key = `dmnote:web:client:${JSON.stringify([
    workerName,
    documentId,
    role,
  ])}`;
  let clientId = sessionStorage.getItem(key) || crypto.randomUUID();
  const locks = navigator.locks;
  let release: (() => void) | null = null;
  if (locks) {
    release = await takeLease(locks, clientId);
    if (!release) {
      clientId = crypto.randomUUID();
      release = await takeLease(locks, clientId);
      if (!release)
        throw webHostError(
          'CLIENT_ID_IN_USE',
          'Cannot acquire browser client identity',
        );
    }
  }
  try {
    sessionStorage.setItem(key, clientId);
  } catch (error) {
    release?.();
    throw error;
  }
  return {
    clientId,
    release() {
      release?.();
      release = null;
    },
    async reacquire() {
      if (release || !locks) return;
      release = await takeLease(locks, clientId);
      if (!release)
        throw webHostError(
          'CLIENT_ID_IN_USE',
          'This browser client identity is in use by another page',
        );
    },
  };
}
