import type { WebAsset } from '../protocol';

export interface ObjectUrlProvider {
  createObjectURL(blob: Blob): string;
  revokeObjectURL(url: string): void;
}

/** 문서 데이터 전달 전에 hydrate해 convertFileSrc의 동기 계약 유지 */
export class AssetUrlRegistry {
  private readonly assets = new Map<string, { asset: WebAsset; url: string }>();
  private readonly retiredUrls = new Set<string>();

  constructor(private readonly urls: ObjectUrlProvider = URL) {}

  hydrate(assets: readonly WebAsset[]): void {
    for (const asset of assets) {
      const current = this.assets.get(asset.key);
      if (
        current?.asset.dataBase64 === asset.dataBase64 &&
        current.asset.mimeType === asset.mimeType
      )
        continue;
      const binary = atob(asset.dataBase64);
      const bytes = Uint8Array.from(binary, (character) =>
        character.charCodeAt(0),
      );
      const url = this.urls.createObjectURL(
        new Blob([bytes], {
          type: asset.mimeType ?? 'application/octet-stream',
        }),
      );
      if (current) this.retiredUrls.add(current.url);
      this.assets.set(asset.key, { asset, url });
    }
  }

  resolve(key: string): string {
    const entry = this.assets.get(key);
    if (entry) return entry.url;
    if (/^(https?:|data:|blob:)/i.test(key)) return key;
    throw new Error(`Web asset is not loaded: ${key}`);
  }

  remove(keys: readonly string[]): void {
    for (const key of keys) {
      const current = this.assets.get(key);
      if (current) this.retiredUrls.add(current.url);
      this.assets.delete(key);
    }
  }

  dispose(): void {
    for (const { url } of this.assets.values()) this.urls.revokeObjectURL(url);
    for (const url of this.retiredUrls) this.urls.revokeObjectURL(url);
    this.assets.clear();
    this.retiredUrls.clear();
  }
}
