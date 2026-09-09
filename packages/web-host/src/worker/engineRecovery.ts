import type { EngineFactory } from '../engine/wasm';
import { collectAssetKeys, type WebStorage } from '../storage/indexedDb';

/** 초기 연결·충돌 재로드의 복구 저장과 receipt 세대 변경을 동일 트랜잭션으로 수행 */
export const restoreSessionEngine = async (
  storage: Pick<WebStorage, 'load' | 'save'>,
  engines: Pick<EngineFactory, 'create'>,
  documentId: string,
  randomId: () => string,
) => {
  const loaded = await storage.load(documentId);
  const engine = engines.create(
    loaded.document?.store,
    loaded.document?.checkpoint,
  );
  try {
    const snapshot = engine.snapshot();
    const store = snapshot.store;
    const retained = collectAssetKeys(store);
    for (const key of loaded.quarantinedKeys ?? [])
      if (!retained.has(key)) {
        delete loaded.assets[key];
        delete loaded.assetModifiedAt[key];
      }
    let version = loaded.document?.version ?? 0;
    const incarnation = engine.recovered
      ? randomId()
      : loaded.document?.incarnation ?? 'original';
    if (
      engine.recovered ||
      !loaded.document ||
      JSON.stringify(loaded.document.store) !== JSON.stringify(store)
    ) {
      version = await storage.save({
        documentId,
        expectedVersion: version,
        incarnation,
        store,
        preservePreviousStore: engine.recovered,
        checkpoint: snapshot.checkpoint,
      });
    }
    return {
      engine,
      store,
      version,
      incarnation,
      assets: loaded.assets,
      assetModifiedAt: loaded.assetModifiedAt,
    };
  } catch (error) {
    engine.dispose();
    throw error;
  }
};
