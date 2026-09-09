import { afterEach, describe, expect, it, vi } from 'vitest';
import { createFrontendFlush } from './frontendFlush';

afterEach(() => vi.useRealTimers());
describe('history frontend flush barrier', () => {
  it('collects both clients, permits their pending writes, then locks until history completes', async () => {
    const send = vi.fn();
    const flush = createFrontendFlush(send);
    const result = flush.begin(['main-id', 'overlay-id']);
    const id = send.mock.calls[0]![2].handshakeId as string;
    expect(send.mock.calls.map((call) => call.slice(0, 2))).toEqual([
      ['main-id', 'app:close-requested'],
      ['overlay-id', 'app:close-requested'],
    ]);
    expect(send.mock.calls[0]![2]).toEqual({
      handshakeId: id,
      action: 'history',
    });
    expect(() => flush.assertMutationAllowed('main-id')).not.toThrow();
    flush.acknowledge('main-id', id);
    expect(() => flush.assertMutationAllowed('main-id')).toThrow(
      expect.objectContaining({ errorCode: 'HISTORY_IN_PROGRESS' }),
    );
    expect(() => flush.assertMutationAllowed('overlay-id')).not.toThrow();
    expect(() => flush.assertMutationAllowed('new-client')).toThrow();
    let resolved = false;
    void result.then(() => {
      resolved = true;
    });
    await Promise.resolve();
    expect(resolved).toBe(false);
    flush.acknowledge('overlay-id', id);
    const ready = await result;
    expect(() => flush.assertMutationAllowed('overlay-id')).toThrow();
    ready.complete();
    expect(send.mock.calls.slice(-2).map((call) => call[1])).toEqual([
      'app:history-flush-released',
      'app:history-flush-released',
    ]);
    expect(() => flush.assertMutationAllowed('main-id')).not.toThrow();
    ready.complete();
    expect(send).toHaveBeenCalledTimes(4);
  });
  it('rejects unknown acknowledgements, concurrent history and cancellation after apply starts', async () => {
    const send = vi.fn();
    const flush = createFrontendFlush(send);
    const result = flush.begin(['a']);
    const id = send.mock.calls[0]![2].handshakeId as string;
    expect(() => flush.acknowledge('other', id)).toThrow();
    expect(() => flush.acknowledge('a', 'stale')).toThrow();
    await expect(flush.begin(['a'])).rejects.toMatchObject({
      code: 'HISTORY_FRONTEND_FLUSH_BUSY',
    });
    flush.acknowledge('a', id);
    const ready = await result;
    expect(() => flush.cancel('a', id)).toThrow();
    ready.complete();
  });
  it('releases locks on client disconnect, cancellation and timeout, with no execution timeout', async () => {
    vi.useFakeTimers();
    for (const cause of ['disconnect', 'cancel', 'timeout'] as const) {
      const send = vi.fn();
      const flush = createFrontendFlush(send);
      const result = flush.begin(['a', 'b']);
      const rejected = expect(result).rejects.toMatchObject({
        retryable: true,
      });
      const id = send.mock.calls[0]![2].handshakeId as string;
      flush.acknowledge('a', id);
      if (cause === 'disconnect') flush.disconnect('b');
      else if (cause === 'cancel') flush.cancel('b', id);
      else await vi.advanceTimersByTimeAsync(10000);
      await rejected;
      expect(() => flush.assertMutationAllowed('a')).not.toThrow();
      expect(send.mock.calls.slice(-2).map((call) => call[1])).toEqual([
        'app:history-flush-released',
        'app:history-flush-released',
      ]);
    }
    const flush = createFrontendFlush(vi.fn());
    const ready = await flush.begin([]);
    await vi.advanceTimersByTimeAsync(20000);
    expect(() => flush.assertMutationAllowed('new')).toThrow();
    ready.complete();
  });
});
