export { connectWebEditor } from './client/connectWebEditor';
export type {
  ConnectWebEditorOptions,
  WebEditorClient,
  WebConnectionState,
} from './client/connectWebEditor';
export type { WebRole, WebHostError } from './protocol';
export type { SharedWorkerConnection } from './client/connectWebEditor';
export { AssetUrlRegistry } from './browser/assetUrls';
export type { ObjectUrlProvider } from './browser/assetUrls';
export { attachBrowserKeyboard } from './browser/keyboard';
export { pickBrowserFiles, downloadBrowserFile } from './browser/files';
export type { FilePickerOptions } from './browser/files';
export type { BrowserFilePicker } from './client/fileCommands';
export { createCssImportFetcher } from './browser/cssImports';
export type {
  CssImportFetcher,
  CssImportResult,
  CssProxyOptions,
} from './browser/cssImports';
export type { WebFile, WebDownload, WebAsset, PortLike } from './protocol';

export { createWebAudioPreview } from './browser/audio';
export type {
  WebAudioPreview,
  WebAudioPreviewOptions,
  WebInputSound,
} from './browser/audio';

export { applyWebOverlayState } from './browser/overlay';
export type {
  WebOverlayState,
  WebOverlayBounds,
  WebOverlayAnchor,
} from './browser/overlay';
