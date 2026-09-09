export interface CssImportResult {
  finalUrl: string;
  text: string;
}
export type CssImportFetcher = (url: string) => Promise<CssImportResult>;

export interface CssProxyOptions {
  proxyUrl: string | URL;
  fetch?: typeof fetch;
}

const MAX_CSS_BYTES = 1024 * 1024;

/** 중계 서버는 최종 URL과 CSS 본문을 JSON으로 반환 */
export function createCssImportFetcher(
  options: CssProxyOptions,
): CssImportFetcher {
  const fetcher = options.fetch ?? fetch;
  return async (url) => {
    const requested = new URL(url);
    if (!['http:', 'https:'].includes(requested.protocol))
      throw new Error('CSS imports require HTTP or HTTPS');
    const endpoint = new URL(options.proxyUrl);
    endpoint.searchParams.set('url', requested.href);
    const abort = new AbortController();
    const timeout = setTimeout(() => abort.abort(), 5000);
    try {
      const response = await fetcher(endpoint, {
        signal: abort.signal,
        credentials: 'omit',
      });
      if (!response.ok)
        throw new Error(`CSS intermediary returned HTTP ${response.status}`);
      if (!response.body)
        throw new Error('CSS intermediary returned no response body');
      const reader = response.body.getReader();
      const chunks: Uint8Array[] = [];
      let length = 0;
      try {
        for (;;) {
          const next = await reader.read();
          if (next.done) break;
          length += next.value.byteLength;
          // JSON escape로 늘어난 크기까지 허용, 해독된 CSS는 아래에서 별도 검사
          if (length > MAX_CSS_BYTES * 6 + 4096)
            throw new Error('CSS intermediary response is too large');
          chunks.push(next.value);
        }
      } catch (error) {
        await reader.cancel().catch(() => {});
        throw error;
      } finally {
        reader.releaseLock();
      }
      const bytes = new Uint8Array(length);
      let offset = 0;
      for (const chunk of chunks) {
        bytes.set(chunk, offset);
        offset += chunk.length;
      }
      const result: unknown = JSON.parse(new TextDecoder().decode(bytes));
      if (
        !result ||
        typeof result !== 'object' ||
        !('text' in result) ||
        typeof result.text !== 'string' ||
        !('finalUrl' in result) ||
        typeof result.finalUrl !== 'string'
      ) {
        throw new Error('Invalid CSS intermediary response');
      }
      if (new TextEncoder().encode(result.text).byteLength > MAX_CSS_BYTES)
        throw new Error('Imported CSS is too large');
      const final = new URL(result.finalUrl);
      if (!['http:', 'https:'].includes(final.protocol))
        throw new Error('Invalid CSS redirect protocol');
      return { finalUrl: final.href, text: result.text };
    } finally {
      clearTimeout(timeout);
    }
  };
}
