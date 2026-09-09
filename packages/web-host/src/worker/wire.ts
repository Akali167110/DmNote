import {
  webHostError,
  type WebAssetMap,
  type WebAsset,
  type WebRole,
  type WebServerMessage,
} from '../protocol';
import { decodeEngineError } from '../engine/wasm';
import type { Client, Session, Invoke } from './types';

const mimeTypes: Record<string, string> = {
  png: 'image/png',
  jpg: 'image/jpeg',
  jpeg: 'image/jpeg',
  gif: 'image/gif',
  webp: 'image/webp',
  avif: 'image/avif',
  bmp: 'image/bmp',
  svg: 'image/svg+xml',
  ttf: 'font/ttf',
  otf: 'font/otf',
  woff: 'font/woff',
  woff2: 'font/woff2',
  wav: 'audio/wav',
  mp3: 'audio/mpeg',
  ogg: 'audio/ogg',
  flac: 'audio/flac',
  aac: 'audio/aac',
  m4a: 'audio/mp4',
  css: 'text/css',
  js: 'text/javascript',
};
export const assetsForClient = (assets: WebAssetMap): WebAsset[] =>
  Object.entries(assets).map(([key, dataBase64]) => ({
    key,
    dataBase64,
    mimeType:
      mimeTypes[key.split('.').pop()!.toLowerCase()] ??
      'application/octet-stream',
  }));
export const encodeBytes = (bytes: ArrayBuffer): string => {
  const data = new Uint8Array(bytes);
  let binary = '';
  for (let offset = 0; offset < data.length; offset += 32768)
    binary += String.fromCharCode(...data.subarray(offset, offset + 32768));
  return btoa(binary);
};
export const responseError = (error: unknown): unknown =>
  error instanceof Error
    ? webHostError(error.name, error.message)
    : decodeEngineError(error);
export const ioError = (error: unknown) => ({
  errorCode: 'IO_ERROR',
  message: error instanceof Error ? error.message : String(error),
  retryable: true,
});
export const fingerprint = async (
  command: string,
  args: Record<string, unknown>,
  files: Invoke['files'],
): Promise<string> => {
  const content = JSON.stringify({
    command,
    args,
    files: files?.map((file) => ({ ...file, bytes: encodeBytes(file.bytes) })),
  });
  const hash = await crypto.subtle.digest(
    'SHA-256',
    new TextEncoder().encode(content),
  );
  return Array.from(new Uint8Array(hash), (byte) =>
    byte.toString(16).padStart(2, '0'),
  ).join('');
};
export const targetRole = (target: unknown): WebRole | undefined => {
  if (target === undefined || target === null) return undefined;
  const label =
    typeof target === 'string'
      ? target
      : typeof target === 'object' && 'label' in target
      ? target.label
      : undefined;
  if (label === 'main' || label === 'overlay') return label;
  throw webHostError(
    'INVALID_EVENT_TARGET',
    'Event target must be main or overlay',
  );
};

export const send = (client: Client, message: WebServerMessage) => {
  try {
    client.port.postMessage(message);
  } catch {
    /* 다음 연결에서 canonical snapshot 재동기화 */
  }
};
export const broadcast = (
  session: Session,
  message: WebServerMessage,
  role?: WebRole,
) => {
  for (const client of session.clients.values())
    if (!role || client.role === role) send(client, message);
};
