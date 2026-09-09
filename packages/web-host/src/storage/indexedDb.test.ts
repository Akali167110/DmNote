import { IDBFactory } from 'fake-indexeddb';
import { describe, it, expect } from 'vitest';
import {
  openWebStorage,
  StorageConflictError,
  ASSET_QUARANTINE_MS,
} from './indexedDb';

describe('브라우저 문서·자산 원자 저장', () => {
  it('복구 원본과 receipt 세대가 후속 정상 저장에도 보존된다', async () => {
    const storage = await openWebStorage({ factory: new IDBFactory() });
    const corrupt = {
      editorRevision: 'damaged',
      asset: '/assets/images/recovery.png',
    };
    await storage.save({
      documentId: 'recovery',
      expectedVersion: 0,
      store: corrupt,
      checkpoint: { invalid: true },
      assetWrites: { '/assets/images/recovery.png': 'YQ==' },
    });
    await storage.save({
      documentId: 'recovery',
      expectedVersion: 1,
      store: { editorRevision: 0 },
      incarnation: 'repaired',
      preservePreviousStore: true,
    });
    await storage.save({
      documentId: 'recovery',
      expectedVersion: 2,
      store: { editorRevision: 1 },
    });
    const loaded = await storage.load('recovery');
    expect(loaded.document).toMatchObject({
      incarnation: 'repaired',
      recoveryStore: corrupt,
      recoveryCheckpoint: { invalid: true },
      store: { editorRevision: 1 },
    });
    await storage.collectUnusedAssets('recovery', new Set());
    expect((await storage.load('recovery')).quarantinedKeys).toEqual([]);
    storage.close();
  });
  it('저장 완료 후 새 연결에서 문서·자산·재전송 receipt를 함께 읽는다', async () => {
    const factory = new IDBFactory();
    const storage = await openWebStorage({ factory, name: 'atomic' });
    const receipt = {
      documentId: 'one',
      requestId: 'req',
      fingerprint: 'abc',
      result: { revision: 1 },
      committedAt: 123,
    };
    expect(
      await storage.save({
        documentId: 'one',
        expectedVersion: 0,
        store: { image: '/assets/images/a.png' },
        assetWrites: { '/assets/images/a.png': 'YWJj' },
        receipt,
      }),
    ).toBe(1);
    storage.close();
    const reopened = await openWebStorage({ factory, name: 'atomic' });
    expect((await reopened.load('one')).assets).toEqual({
      '/assets/images/a.png': 'YWJj',
    });
    expect((await reopened.load('one')).document?.store).toEqual({
      image: '/assets/images/a.png',
    });
    expect(await reopened.receipt('one', 'req')).toEqual(receipt);
    expect((await reopened.load('two')).document).toBeUndefined();
    expect((await reopened.load('two')).assets).toEqual({});
    expect(await reopened.receipt('two', 'req')).toBeUndefined();
    reopened.close();
  });
  it('잘못된 쓰기가 중간에 실패하면 document와 앞서 enqueue된 자산도 롤백한다', async () => {
    const storage = await openWebStorage({ factory: new IDBFactory() });
    await storage.save({
      documentId: 'doc',
      expectedVersion: 0,
      store: { revision: 0 },
    });
    await expect(
      storage.save({
        documentId: 'doc',
        expectedVersion: 1,
        store: { revision: 1 },
        assetWrites: { '/assets/images/a.png': 'YWJj' },
        receipt: {
          documentId: 'doc',
          requestId: 'bad',
          fingerprint: 'x',
          result: () => {},
          committedAt: 0,
        },
      }),
    ).rejects.toThrow();
    expect(await storage.load('doc')).toMatchObject({
      document: { version: 1, store: { revision: 0 } },
      assets: {},
    });
    expect(await storage.receipt('doc', 'bad')).toBeUndefined();
    storage.close();
  });
  it('경합한 writer는 이전 version으로 새 변경을 덮어쓰지 않는다', async () => {
    const factory = new IDBFactory();
    const a = await openWebStorage({ factory });
    const b = await openWebStorage({ factory });
    await a.save({
      documentId: 'doc',
      expectedVersion: 0,
      store: { value: 'first' },
    });
    const results = await Promise.allSettled([
      a.save({ documentId: 'doc', expectedVersion: 1, store: { value: 'a' } }),
      b.save({ documentId: 'doc', expectedVersion: 1, store: { value: 'b' } }),
    ]);
    expect(
      results.filter((result) => result.status === 'fulfilled'),
    ).toHaveLength(1);
    const rejected = results.find((result) => result.status === 'rejected');
    expect(rejected?.reason).toBeInstanceOf(StorageConflictError);
    expect((await a.load('doc')).document?.version).toBe(2);
    a.close();
    b.close();
  });
  it('자산은 30일 격리하고 현재·직전 문서와 보존 참조를 보호한다', async () => {
    let now = 1;
    const storage = await openWebStorage({
      factory: new IDBFactory(),
      now: () => now,
    });
    const old = '/assets/images/old.png';
    const current = '/assets/fonts/current.woff2';
    const orphan = '/assets/sounds/orphan.wav';
    const history = '/assets/scripts/history.js';
    await storage.save({
      documentId: 'doc',
      expectedVersion: 0,
      store: { asset: old },
      assetWrites: {
        [old]: 'YQ==',
        [current]: 'Yg==',
        [orphan]: 'Yw==',
        [history]: 'ZA==',
      },
    });
    await storage.save({
      documentId: 'doc',
      expectedVersion: 1,
      store: { asset: current },
      assetDeletes: [orphan],
    });
    await storage.collectUnusedAssets('doc', new Set([history]));
    expect(Object.keys((await storage.load('doc')).assets)).toHaveLength(4);
    now += ASSET_QUARANTINE_MS;
    await storage.collectUnusedAssets('doc', new Set([history]));
    expect((await storage.load('doc')).assets).toEqual({
      [old]: 'YQ==',
      [current]: 'Yg==',
      [history]: 'ZA==',
    });
    await storage.save({
      documentId: 'doc',
      expectedVersion: 2,
      store: { asset: current },
    });
    await storage.collectUnusedAssets('doc', new Set());
    now += ASSET_QUARANTINE_MS;
    await storage.collectUnusedAssets('doc', new Set());
    expect((await storage.load('doc')).assets).toEqual({ [current]: 'Yg==' });
    storage.close();
  });
  it('격리 중인 자산을 다시 참조하면 격리를 해제한다', async () => {
    let now = 1;
    const storage = await openWebStorage({
      factory: new IDBFactory(),
      now: () => now,
    });
    const key = '/assets/images/reused.png';
    await storage.save({
      documentId: 'doc',
      expectedVersion: 0,
      store: {},
      assetWrites: { [key]: 'YQ==' },
    });
    await storage.collectUnusedAssets('doc', new Set());
    now += ASSET_QUARANTINE_MS;
    await storage.save({
      documentId: 'doc',
      expectedVersion: 1,
      store: { image: key },
    });
    await storage.collectUnusedAssets('doc', new Set());
    expect((await storage.load('doc')).assets[key]).toBe('YQ==');
    storage.close();
  });
});
