import { afterEach, describe, expect, it, vi } from 'vitest';
import { downloadBrowserFile, pickBrowserFiles } from './files';

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});
describe('browser file lifecycle', () => {
  it('reads selected bytes and removes input on success, cancellation and abort', async () => {
    vi.spyOn(HTMLInputElement.prototype, 'click').mockImplementation(() => {});
    const selected = pickBrowserFiles({ accept: '.json', multiple: true });
    const input = document.querySelector(
      'input[type=file]',
    ) as HTMLInputElement;
    expect(input.multiple).toBe(true);
    const bytes = new TextEncoder().encode('preset').buffer;
    Object.defineProperty(input, 'files', {
      value: [
        {
          name: 'preset.json',
          type: 'application/json',
          arrayBuffer: () => Promise.resolve(bytes),
        },
      ],
    });
    input.dispatchEvent(new Event('change'));
    await expect(selected).resolves.toEqual([
      { name: 'preset.json', mimeType: 'application/json', bytes },
    ]);
    expect(document.querySelector('input[type=file]')).toBeNull();
    const canceled = pickBrowserFiles({ accept: '.css' });
    document
      .querySelector('input[type=file]')!
      .dispatchEvent(new Event('cancel'));
    await expect(canceled).resolves.toEqual([]);
    const abort = new AbortController();
    const pending = pickBrowserFiles({ accept: '.js', signal: abort.signal });
    abort.abort();
    await expect(pending).rejects.toMatchObject({ name: 'AbortError' });
    expect(document.querySelector('input[type=file]')).toBeNull();
  });
  it('propagates blocked activation and revokes download URLs after the click', async () => {
    vi.spyOn(HTMLInputElement.prototype, 'click').mockImplementation(() => {
      throw new Error('activation required');
    });
    await expect(pickBrowserFiles({ accept: '.json' })).rejects.toThrow(
      'activation required',
    );
    expect(document.querySelector('input[type=file]')).toBeNull();
    vi.useFakeTimers();
    const createObjectURL = vi.fn(() => 'blob:download');
    const revokeObjectURL = vi.fn();
    vi.stubGlobal(
      'URL',
      class extends URL {
        static createObjectURL = createObjectURL;
        static revokeObjectURL = revokeObjectURL;
      },
    );
    let downloadName = '';
    vi.spyOn(HTMLAnchorElement.prototype, 'click').mockImplementation(function (
      this: HTMLAnchorElement,
    ) {
      downloadName = this.download;
      expect(revokeObjectURL).not.toHaveBeenCalled();
    });
    downloadBrowserFile({
      name: 'preset.json',
      mimeType: 'application/json',
      bytes: new ArrayBuffer(1),
    });
    expect(downloadName).toBe('preset.json');
    expect(document.querySelector('a[download]')).toBeNull();
    await vi.runAllTimersAsync();
    expect(revokeObjectURL).toHaveBeenCalledWith('blob:download');
  });
});
