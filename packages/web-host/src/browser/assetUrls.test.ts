import { describe, expect, it, vi } from 'vitest';
import { AssetUrlRegistry } from './assetUrls';

describe('웹 자산 URL 수명', () => {
  it('같은 bytes는 URL을 재사용하고 이전 렌더 URL은 세션 해제까지 유지한다', () => {
    let next = 0;
    const urls = {
      createObjectURL: vi.fn(() => `blob:${++next}`),
      revokeObjectURL: vi.fn(),
    };
    const registry = new AssetUrlRegistry(urls);
    registry.hydrate([
      { key: '/assets/a', dataBase64: 'YQ==', mimeType: 'image/png' },
    ]);
    expect(registry.resolve('/assets/a')).toBe('blob:1');
    registry.hydrate([
      { key: '/assets/a', dataBase64: 'YQ==', mimeType: 'image/png' },
    ]);
    expect(urls.createObjectURL).toHaveBeenCalledTimes(1);
    registry.hydrate([
      { key: '/assets/a', dataBase64: 'Yg==', mimeType: 'image/png' },
    ]);
    expect(registry.resolve('/assets/a')).toBe('blob:2');
    registry.remove(['/assets/a']);
    expect(() => registry.resolve('/assets/a')).toThrow('not loaded');
    expect(urls.revokeObjectURL).not.toHaveBeenCalled();
    registry.dispose();
    expect(urls.revokeObjectURL.mock.calls.flat().sort()).toEqual([
      'blob:1',
      'blob:2',
    ]);
  });
});
