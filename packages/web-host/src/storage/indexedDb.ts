import type { WebAssetMap } from '../protocol.js';

export interface StoredDocument {
  id: string;
  incarnation?: string;
  version: number;
  store: Record<string, unknown>;
  previousStore?: Record<string, unknown>;
  recoveryStore?: Record<string, unknown>;
  recoveryCheckpoint?: Record<string, unknown>;
  checkpoint?: Record<string, unknown>;
}
export interface StoredReceipt {
  documentId: string;
  requestId: string;
  fingerprint: string;
  incarnation?: string;
  result: unknown;
  committedAt: number;
}
interface StoredAsset {
  documentId: string;
  key: string;
  dataBase64: string;
  quarantinedAt?: number;
  modifiedAtMs: number;
}
export interface StoredWorkspace {
  document?: StoredDocument;
  assets: WebAssetMap;
  assetModifiedAt: Record<string, number>;
  quarantinedKeys: string[];
}
export interface SaveWorkspace {
  documentId: string;
  expectedVersion: number;
  incarnation?: string;
  preservePreviousStore?: boolean;
  store: Record<string, unknown>;
  assetWrites?: WebAssetMap;
  assetDeletes?: string[];
  receipt?: StoredReceipt;
  checkpoint?: Record<string, unknown>;
}
export interface WebStorage {
  load(documentId: string): Promise<StoredWorkspace>;
  receipt(
    documentId: string,
    requestId: string,
  ): Promise<StoredReceipt | undefined>;
  save(change: SaveWorkspace): Promise<number>;
  collectUnusedAssets(
    documentId: string,
    retained: ReadonlySet<string>,
  ): Promise<void>;
  close(): void;
}
export class StorageConflictError extends Error {
  constructor(readonly current: StoredDocument | undefined) {
    super(
      '다른 실행 세션에서 이 작업을 변경했습니다. 최신 상태를 다시 불러와야 합니다.',
    );
    this.name = 'StorageConflictError';
  }
}
export const ASSET_QUARANTINE_MS = 30 * 24 * 60 * 60 * 1000;
const requestValue = <T>(request: IDBRequest<T>): Promise<T> =>
  new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
const transactionDone = (transaction: IDBTransaction): Promise<void> =>
  new Promise((resolve, reject) => {
    transaction.oncomplete = () => resolve();
    transaction.onabort = () =>
      reject(
        transaction.error ??
          new DOMException('IndexedDB transaction aborted', 'AbortError'),
      );
    transaction.onerror = () => {
      /* abort에서 최종 실패 전달 */
    };
  });
const openDatabase = (
  factory: IDBFactory,
  name: string,
): Promise<IDBDatabase> =>
  new Promise((resolve, reject) => {
    let blocked = false;
    const request = factory.open(name, 1);
    request.onupgradeneeded = () => {
      const db = request.result;
      db.createObjectStore('documents', { keyPath: 'id' });
      const assets = db.createObjectStore('assets', {
        keyPath: ['documentId', 'key'],
      });
      assets.createIndex('documentId', 'documentId');
      const receipts = db.createObjectStore('receipts', {
        keyPath: ['documentId', 'requestId'],
      });
      receipts.createIndex('documentId', 'documentId');
    };
    request.onsuccess = () => {
      const db = request.result;
      if (blocked) {
        db.close();
        return;
      }
      db.onversionchange = () => db.close();
      resolve(db);
    };
    request.onerror = () => reject(request.error);
    request.onblocked = () => {
      blocked = true;
      reject(
        new Error(
          '기존 화면이 저장소 업데이트를 차단하고 있습니다. 해당 화면을 닫고 다시 연결해 주세요.',
        ),
      );
    };
  });
export interface IndexedDbOptions {
  factory?: IDBFactory;
  name?: string;
  now?: () => number;
}
export const openWebStorage = async ({
  factory = indexedDB,
  name = 'dmnote-web-editor-v1',
  now = Date.now,
}: IndexedDbOptions = {}): Promise<WebStorage> => {
  const db = await openDatabase(factory, name);
  const writeTransaction = (stores: string[]) =>
    db.transaction(stores, 'readwrite', { durability: 'strict' });
  return {
    async load(documentId) {
      const tx = db.transaction(['documents', 'assets'], 'readonly');
      const done = transactionDone(tx);
      const documentRequest = requestValue(
        tx.objectStore('documents').get(documentId) as IDBRequest<
          StoredDocument | undefined
        >,
      );
      const assetsRequest = requestValue(
        tx
          .objectStore('assets')
          .index('documentId')
          .getAll(documentId) as IDBRequest<StoredAsset[]>,
      );
      const [document, records] = await Promise.all([
        documentRequest,
        assetsRequest,
        done,
      ]);
      return {
        document,
        quarantinedKeys: records
          .filter((asset) => asset.quarantinedAt !== undefined)
          .map((asset) => asset.key),
        assets: Object.fromEntries(
          records.map((asset) => [asset.key, asset.dataBase64]),
        ),
        assetModifiedAt: Object.fromEntries(
          records.map((asset) => [asset.key, asset.modifiedAtMs]),
        ),
      };
    },
    async receipt(documentId, requestId) {
      const tx = db.transaction('receipts', 'readonly');
      const done = transactionDone(tx);
      const [result] = await Promise.all([
        requestValue(
          tx.objectStore('receipts').get([documentId, requestId]) as IDBRequest<
            StoredReceipt | undefined
          >,
        ),
        done,
      ]);
      return result;
    },
    async save(change) {
      const tx = writeTransaction(['documents', 'assets', 'receipts']);
      const done = transactionDone(tx);
      // 실패 핸들러를 즉시 연결해 요청 실패와 abort가 함께 발생해도 미처리 rejection 방지
      void done.catch(() => undefined);
      try {
        const documents = tx.objectStore('documents');
        const before = await requestValue(
          documents.get(change.documentId) as IDBRequest<
            StoredDocument | undefined
          >,
        );
        if ((before?.version ?? 0) !== change.expectedVersion)
          throw new StorageConflictError(before);
        const version = change.expectedVersion + 1;
        if (!Number.isSafeInteger(version))
          throw new Error('저장 revision 한도를 초과했습니다.');
        documents.put({
          id: change.documentId,
          incarnation: change.incarnation ?? before?.incarnation ?? 'original',
          version,
          store: change.store,
          checkpoint: change.checkpoint,
          ...(before
            ? {
                recoveryStore: change.preservePreviousStore
                  ? before.store
                  : before.recoveryStore,
                recoveryCheckpoint: change.preservePreviousStore
                  ? before.checkpoint
                  : before.recoveryCheckpoint,
              }
            : {}),
          ...(before
            ? {
                previousStore: change.preservePreviousStore
                  ? before.previousStore ?? before.store
                  : before.store,
              }
            : {}),
        } satisfies StoredDocument);
        const assets = tx.objectStore('assets');
        for (const [key, dataBase64] of Object.entries(
          change.assetWrites ?? {},
        )) {
          assets.put({
            documentId: change.documentId,
            key,
            dataBase64,
            modifiedAtMs: now(),
          } satisfies StoredAsset);
        }
        for (const key of change.assetDeletes ?? []) {
          const request = assets.get([change.documentId, key]) as IDBRequest<
            StoredAsset | undefined
          >;
          request.onsuccess = () => {
            if (request.result)
              assets.put({
                ...request.result,
                quarantinedAt: request.result.quarantinedAt ?? now(),
              });
          };
        }
        if (change.receipt) tx.objectStore('receipts').put(change.receipt);
        await done;
        return version;
      } catch (error) {
        try {
          tx.abort();
        } catch {
          /* 이미 완료되거나 중단된 트랜잭션 */
        }
        await done.catch(() => undefined);
        throw error;
      }
    },
    async collectUnusedAssets(documentId, retained) {
      // 호스트가 실행 세션·히스토리 사용 종료 후에만 호출
      const tx = writeTransaction(['assets', 'documents']);
      const done = transactionDone(tx);
      void done.catch(() => undefined);
      const current = await requestValue(
        tx.objectStore('documents').get(documentId) as IDBRequest<
          StoredDocument | undefined
        >,
      );
      const protectedKeys = collectAssetKeys(current?.store, new Set(retained));
      collectAssetKeys(current?.previousStore, protectedKeys);
      collectAssetKeys(current?.recoveryStore, protectedKeys);
      const cursorRequest = tx
        .objectStore('assets')
        .index('documentId')
        .openCursor(documentId);
      cursorRequest.onsuccess = () => {
        const cursor = cursorRequest.result;
        if (!cursor) return;
        const asset = cursor.value as StoredAsset;
        if (protectedKeys.has(asset.key)) {
          if (asset.quarantinedAt !== undefined) {
            const { quarantinedAt: _quarantinedAt, ...active } = asset;
            cursor.update(active);
          }
        } else if (asset.quarantinedAt === undefined)
          cursor.update({ ...asset, quarantinedAt: now() });
        else if (now() - asset.quarantinedAt >= ASSET_QUARANTINE_MS)
          cursor.delete();
        cursor.continue();
      };
      await done;
    },
    close: () => db.close(),
  };
};
export const collectAssetKeys = (
  value: unknown,
  result = new Set<string>(),
): Set<string> => {
  if (typeof value === 'string' && value.startsWith('/assets/'))
    result.add(value);
  else if (Array.isArray(value))
    for (const entry of value) collectAssetKeys(entry, result);
  else if (value && typeof value === 'object')
    for (const entry of Object.values(value)) collectAssetKeys(entry, result);
  return result;
};
