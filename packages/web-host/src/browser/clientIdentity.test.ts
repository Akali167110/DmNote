import { afterEach, describe, expect, it, vi } from 'vitest';
import { createBrowserClientIdentity } from './clientIdentity';

const fakeLocks = () => {
  const held = new Set<string>();
  return {
    request: vi.fn(
      async (
        name: string,
        _options: unknown,
        callback: (lock: unknown) => unknown,
      ) => {
        if (held.has(name)) return callback(null);
        held.add(name);
        try {
          return await callback({ name });
        } finally {
          held.delete(name);
        }
      },
    ),
  } as unknown as LockManager;
};
afterEach(() => {
  sessionStorage.clear();
  vi.restoreAllMocks();
});
describe('browser client identity lease', () => {
  it('reuses a released page identity but separates cloned sessionStorage tabs', async () => {
    Object.defineProperty(navigator, 'locks', {
      value: fakeLocks(),
      configurable: true,
    });
    const first = await createBrowserClientIdentity('doc', 'main', 'worker');
    const cloned = await createBrowserClientIdentity('doc', 'main', 'worker');
    expect(cloned.clientId).not.toBe(first.clientId);
    cloned.release();
    await Promise.resolve();
    const reloaded = await createBrowserClientIdentity('doc', 'main', 'worker');
    expect(reloaded.clientId).toBe(cloned.clientId);
    reloaded.release();
    first.release();
    await Promise.resolve();
    await first.reacquire();
    first.release();
    Reflect.deleteProperty(navigator, 'locks');
  });
});
