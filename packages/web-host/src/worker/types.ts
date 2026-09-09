import type {
  PortLike,
  WebAssetMap,
  WebClientMessage,
  WebRole,
} from '../protocol';
import type { EditorEngine, KeyboardMatcher } from '../engine/wasm';
import type { createFrontendFlush } from './frontendFlush';
import type { createPreviewBroker } from './preview';
import type { createWebOverlayPreview } from './overlay';

export type Invoke = Extract<WebClientMessage, { type: 'invoke' }>;
export interface Client {
  id: string;
  realmId: string;
  role: WebRole;
  port: PortLike;
  rawSubscriptions: number;
  rawReceipts: Map<string, { command: string; count: number }>;
  keys: Map<string, { candidates: string[]; downAt: number }>;
}
export interface Session {
  id: string;
  documentId: string;
  incarnation: string;
  engine: EditorEngine;
  keyboard: KeyboardMatcher;
  assets: WebAssetMap;
  assetModifiedAt: Record<string, number>;
  version: number;
  clients: Map<string, Client>;
  queue: Promise<void>;
  releaseLock: () => void;
  flush: ReturnType<typeof createFrontendFlush>;
  closing: boolean;
  skipAssetSweep: boolean;
  overlay: ReturnType<typeof createWebOverlayPreview>;
  preview: ReturnType<typeof createPreviewBroker>;
}

export type InvokeEngine = (
  session: Session,
  client: Client,
  message: Invoke,
) => Promise<unknown>;
