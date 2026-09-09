import { describe, expect, it, vi } from 'vitest';
import { createCssImportFetcher } from './cssImports';

describe('CSS 중계 연결', () => {
  it('설정한 중계 URL로 요청하고 상대 자산 기준 finalUrl을 보존한다', async () => {
    const fetcher = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          finalUrl: 'https://cdn.test/final/style.css',
          text: '.key { color: red; }',
        }),
      ),
    );
    const read = createCssImportFetcher({
      proxyUrl: 'https://proxy.test/css',
      fetch: fetcher,
    });
    const result = await read('https://origin.test/a.css');
    expect(result.finalUrl).toBe('https://cdn.test/final/style.css');
    expect(String(fetcher.mock.calls[0][0])).toBe(
      'https://proxy.test/css?url=https%3A%2F%2Forigin.test%2Fa.css',
    );
    expect(fetcher.mock.calls[0][1]).toMatchObject({ credentials: 'omit' });
  });
  it('초과 본문과 잘못된 redirect 프로토콜을 거부한다', async () => {
    const oversized = createCssImportFetcher({
      proxyUrl: 'https://proxy.test/',
      fetch: vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            finalUrl: 'https://a.test/a.css',
            text: 'x'.repeat(1024 * 1024 + 1),
          }),
        ),
      ),
    });
    await expect(oversized('https://a.test/a.css')).rejects.toThrow(
      'too large',
    );
    const invalid = createCssImportFetcher({
      proxyUrl: 'https://proxy.test/',
      fetch: vi
        .fn()
        .mockResolvedValue(
          new Response(JSON.stringify({ finalUrl: 'file:///a.css', text: '' })),
        ),
    });
    await expect(invalid('https://a.test/a.css')).rejects.toThrow(
      'redirect protocol',
    );
  });
});
