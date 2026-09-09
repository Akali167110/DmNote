import { connectWebEditor, type WebEditorClient } from '@dmnote/web-host';
import { Channel } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import '@dmnote/editor/style.css';

const parameters = new URLSearchParams(location.search);
const events: Array<{ event: string; payload: unknown }> = [];
const downloads: Array<{ name: string; content: string }> = [];
const files: Array<{ name: string; mimeType: string; bytes: ArrayBuffer }> = [];
const host = await connectWebEditor({
  workerUrl: '/engine/worker.js',
  documentId: parameters.get('document') ?? 'integration',
  role: parameters.get('role') === 'overlay' ? 'overlay' : 'main',
  pickFiles: async () => files.splice(0),
  download: (file) => {
    downloads.push({
      name: file.name,
      content: new TextDecoder().decode(file.bytes),
    });
  },
});
await host.installEditorApi();
const names = [
  'editor:committed',
  'plugin-bridge:message',
  'keys:state',
  'keys:counter',
  'history:status',
  'customTabs:changed',
  'app:close-requested',
  'app:history-flush-released',
  'obs:resync',
  'pluginInstances:changed',
  'css:content',
  'tabCss:changed',
  'js:content',
];
let mounted = false;
await Promise.all(
  names.map((name) =>
    listen(name, (event) => {
      events.push({ event: name, payload: event.payload });
      if (name === 'app:close-requested' && !mounted)
        void host.invoke('app_quit_after_editor_flush', {
          handshakeId: (event.payload as { handshakeId: string }).handshakeId,
        });
    }),
  ),
);
const previews: unknown[] = [];
const channel = new Channel((payload) => previews.push(payload));
await host.invoke('editor_preview_subscribe', { channel });
const mount = async () => {
  const [{ createElement }, { createRoot }, { Grid, I18nProvider }, runtime] =
    await Promise.all([
      import('react'),
      import('react-dom/client'),
      import('@dmnote/editor/editor'),
      import('@dmnote/editor/runtime'),
    ]);
  const Surface = () => {
    runtime.useAppBootstrap();
    runtime.useCustomJsInjection();
    runtime.useCustomCssInjection({
      scopeSelector: runtime.USER_CSS_SCOPE_SELECTOR,
    });
    const state = runtime.useKeyStore();
    return createElement(
      'div',
      { 'data-bootstrapped': String(state.isBootstrapped) },
      createElement(Grid, {
        keyMappings: state.keyMappings,
        positions: state.positions,
        color: '#ffffff',
        activeTool: 'select',
        onUndo: () => {},
        onRedo: () => {},
        toolbarAddRequest: null,
        onToolbarAddConsumed: undefined,
        isNoteSettingOpen: false,
        setIsNoteSettingOpen: () => {},
        showAlert: () => {},
        showConfirm: () => {},
      }),
    );
  };
  mounted = true;
  createRoot(document.getElementById('root')!).render(
    createElement(I18nProvider, null, createElement(Surface)),
  );
};
Object.assign(window, {
  webTest: { host, events, downloads, files, previews, mount },
  ready: true,
});

export interface WebConsumerTest {
  host: WebEditorClient;
  events: typeof events;
  downloads: typeof downloads;
  files: typeof files;
  previews: unknown[];
  mount(): Promise<void>;
}
