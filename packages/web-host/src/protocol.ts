export const WEB_PROTOCOL_VERSION = 1;
export type WebRole = 'main' | 'overlay';
export type WebAssetMap = Record<string, string>;
export interface WebAsset {
  key: string;
  dataBase64: string;
  mimeType?: string;
}
export interface WebFile {
  name: string;
  mimeType: string;
  bytes: ArrayBuffer;
}
export type WebDownload = WebFile;
export interface WebHostError {
  code: string;
  message: string;
  retryable: boolean;
  details?: unknown;
}
export type WebClientMessage =
  | {
      type: 'connect';
      version: 1;
      requestId: string;
      documentId: string;
      role: WebRole;
      clientId: string;
      realmId: string;
    }
  | {
      type: 'invoke';
      requestId: string;
      command: string;
      args: Record<string, unknown>;
      files?: WebFile[];
    }
  | {
      type: 'emit';
      requestId: string;
      event: string;
      payload: unknown;
      target?: unknown;
    }
  | {
      type: 'input';
      device?: 'keyboard' | 'mouse';
      code: string;
      key: string;
      location: number;
      pressed: boolean;
      timestamp: number;
      globalKey?: string;
    }
  | { type: 'disconnect' };
export type WebServerMessage =
  | {
      type: 'ready';
      requestId: string;
      documentId: string;
      role: WebRole;
      clientId: string;
      sessionId: string;
      generation: string;
      pluginAuthorityResetRequired?: boolean;
      assets: WebAsset[];
    }
  | {
      type: 'response';
      requestId: string;
      ok: true;
      result: unknown;
      download?: WebDownload;
      assets?: WebAsset[];
    }
  | { type: 'response'; requestId: string; ok: false; error: unknown }
  | { type: 'event'; event: string; payload: unknown }
  | { type: 'callback'; callbackId: number; payload: unknown }
  | { type: 'assets'; assets: WebAsset[]; removed?: string[] }
  | { type: 'error'; error: unknown };
export interface PortLike {
  postMessage(message: unknown): void;
  addEventListener(
    type: 'message' | 'messageerror',
    listener: (event: MessageEvent) => void,
  ): void;
  removeEventListener(
    type: 'message' | 'messageerror',
    listener: (event: MessageEvent) => void,
  ): void;
  start(): void;
  close(): void;
}
export interface WebEvent {
  event: string;
  payload: unknown;
}
export const webHostError = (
  code: string,
  message: string,
  retryable = false,
  details?: unknown,
): WebHostError => ({
  code,
  message,
  retryable,
  ...(details === undefined ? {} : { details }),
});
