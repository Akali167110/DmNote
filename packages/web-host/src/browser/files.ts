import type { WebDownload, WebFile } from '../protocol';

export interface FilePickerOptions {
  accept: string;
  multiple?: boolean;
  signal?: AbortSignal;
}

/** 취소는 빈 배열, 브라우저가 파일 선택을 거부한 경우는 명시적 오류 */
export function pickBrowserFiles(
  options: FilePickerOptions,
): Promise<WebFile[]> {
  return new Promise((resolve, reject) => {
    const input = document.createElement('input');
    input.type = 'file';
    input.accept = options.accept;
    input.multiple = options.multiple ?? false;
    input.style.display = 'none';
    let settled = false;
    const cleanup = () => {
      input.removeEventListener('change', change);
      input.removeEventListener('cancel', cancel);
      options.signal?.removeEventListener('abort', abort);
      input.remove();
    };
    const settle = (files: WebFile[] | null, error?: unknown) => {
      if (settled) return;
      settled = true;
      cleanup();
      if (files === null) reject(error);
      else resolve(files);
    };
    const cancel = () => settle([]);
    const abort = () =>
      settle(null, new DOMException('File selection aborted', 'AbortError'));
    const change = () => {
      void Promise.all(
        Array.from(
          input.files ?? [],
          async (file): Promise<WebFile> => ({
            name: file.name,
            mimeType: file.type,
            bytes: await file.arrayBuffer(),
          }),
        ),
      ).then(settle, (error: unknown) => settle(null, error));
    };
    if (options.signal?.aborted) {
      abort();
      return;
    }
    input.addEventListener('change', change);
    input.addEventListener('cancel', cancel);
    options.signal?.addEventListener('abort', abort, { once: true });
    document.body.appendChild(input);
    try {
      if (typeof input.showPicker === 'function') input.showPicker();
      else input.click();
    } catch (error) {
      settle(null, error);
    }
  });
}

export function downloadBrowserFile(file: WebDownload): void {
  const url = URL.createObjectURL(
    new Blob([file.bytes], { type: file.mimeType }),
  );
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = file.name;
  anchor.style.display = 'none';
  document.body.appendChild(anchor);
  try {
    anchor.click();
  } finally {
    anchor.remove();
    // 브라우저가 다운로드 URL을 읽을 다음 작업까지 유지
    setTimeout(() => URL.revokeObjectURL(url), 0);
  }
}
