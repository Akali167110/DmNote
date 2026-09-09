import { describe, expect, it, vi } from 'vitest';
import { restoreSessionEngine } from './engineRecovery';
import type { EditorEngine } from '../engine/wasm';
import type { SaveWorkspace } from '../storage/indexedDb';

const store = {
  keys: {},
  selectedKeyType: '4key',
  image: '/assets/retained.png',
};
const checkpoint = { authorityAvailable: false };
function fixture(recovered: boolean) {
  const dispose = vi.fn();
  const unused = () => {
    throw new Error('unexpected engine command during recovery');
  };
  const engine: EditorEngine = {
    recovered,
    snapshot: () => ({
      store,
      history: {},
      pluginModelRevision: 0,
      authorityGeneration: 0,
      checkpoint,
    }),
    read: unused,
    prepare: unused,
    confirm: unused,
    discard: unused,
    dispose,
  };
  const load = vi.fn(async () => ({
    document: {
      id: 'doc',
      incarnation: 'before',
      version: 4,
      store,
      checkpoint,
    },
    assets: { '/assets/retained.png': 'aGk=', '/assets/retired.png': 'aGk=' },
    assetModifiedAt: {},
    quarantinedKeys: ['/assets/retained.png', '/assets/retired.png'],
  }));
  const save = vi.fn(async (_change: SaveWorkspace) => 5);
  return {
    engine,
    dispose,
    storage: { load, save },
    engines: { create: vi.fn(() => engine) },
  };
}
describe('initial and conflict reload recovery boundary', () => {
  it('persists a fresh receipt incarnation and archive even when only the checkpoint needed recovery', async () => {
    const { storage, engines } = fixture(true);
    const restored = await restoreSessionEngine(
      storage,
      engines,
      'doc',
      () => 'recovered',
    );
    expect(storage.save).toHaveBeenCalledExactlyOnceWith({
      documentId: 'doc',
      expectedVersion: 4,
      incarnation: 'recovered',
      store,
      checkpoint,
      preservePreviousStore: true,
    });
    expect(restored).toMatchObject({
      version: 5,
      incarnation: 'recovered',
      assets: { '/assets/retained.png': 'aGk=' },
    });
    expect(restored.assets).not.toHaveProperty('/assets/retired.png');
  });
  it('preserves ordinary restart incarnation without writing an unchanged checkpoint', async () => {
    const { storage, engines } = fixture(false);
    const restored = await restoreSessionEngine(
      storage,
      engines,
      'doc',
      () => 'unexpected',
    );
    expect(restored.incarnation).toBe('before');
    expect(restored.version).toBe(4);
    expect(storage.save).not.toHaveBeenCalled();
  });
  it('frees the replacement engine if recovery persistence fails', async () => {
    const { storage, engines, dispose } = fixture(true);
    storage.save.mockRejectedValueOnce(new Error('quota'));
    await expect(
      restoreSessionEngine(storage, engines, 'doc', () => 'recovered'),
    ).rejects.toThrow('quota');
    expect(dispose).toHaveBeenCalledOnce();
  });
});
