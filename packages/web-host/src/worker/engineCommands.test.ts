import { describe, expect, it, vi } from 'vitest';
import { readCommandReceipt } from './engineCommands';
import type { Session } from './types';
import type { WebStorage } from '../storage/indexedDb';
import { fingerprint } from './wire';

const message = {
  type: 'invoke' as const,
  command: 'editor_commit',
  requestId: 'before-recovery',
  args: { request: { value: 7 } },
};
describe('recovery receipt incarnation', () => {
  it('rejects an old incarnation without replaying its saved success or reapplying its mutation', async () => {
    const read = vi.fn(() => ({ revision: 12 }));
    const session = {
      documentId: 'doc',
      incarnation: 'recovered',
      engine: { read },
    } as unknown as Session;
    const storage = {
      receipt: vi.fn(async () => ({
        documentId: 'doc',
        requestId: message.requestId,
        fingerprint: await fingerprint(
          message.command,
          message.args,
          undefined,
        ),
        result: { revision: 11 },
        committedAt: 0,
      })),
    } as unknown as WebStorage;
    await expect(readCommandReceipt(storage, session, message)).rejects.toEqual(
      {
        errorCode: 'REVISION_CONFLICT',
        message: 'editor revision conflict',
        details: { currentRevision: 12 },
        retryable: true,
      },
    );
    expect(read).toHaveBeenCalledWith('editor_get', {});
    session.incarnation = 'original';
    await expect(
      readCommandReceipt(storage, session, message),
    ).resolves.toMatchObject({ result: { revision: 11 } });
  });
});
