import { connectWebEditor } from '@dmnote/web-host';
import '@dmnote/editor/style.css';

const params = new URLSearchParams(location.search);
const role = params.get('role') === 'overlay' ? 'overlay' : 'main';
const documentId = params.get('document') ?? 'web-demo';
const host = await connectWebEditor({
  workerUrl: '/engine/worker.js',
  documentId,
  role,
});
await host.installEditorApi();
const React = await import('react');
const { createRoot } = await import('react-dom/client');
const ui = await import('@dmnote/editor/editor');
const runtime = await import('@dmnote/editor/runtime');
const { default: WebDemoPreview } = await import('./web-demo-preview');
const { useState, useEffect, useRef } = React;

if (role === 'main') {
  runtime.usePropertiesPanelStore.getState().setCanvasPanelOpen(true);
}

const style = document.createElement('style');
style.textContent = `
html,body,#root{margin:0;width:100%;height:100%;overflow:hidden}
body{--dmn-modal-top:0px;--dmn-modal-bottom:0px;background:#101114;color:#eee;font-family:system-ui,sans-serif}
.demo-shell{height:100%;display:flex;flex-direction:column}
.demo-button{font:inherit;font-size:12px;line-height:20px;color:#e4e6eb;background:#ffffff0c;border:1px solid #ffffff18;border-radius:7px;padding:5px 11px;cursor:pointer}
.demo-button:hover{background:#ffffff18}.demo-button:disabled{opacity:.4;cursor:wait}
.demo-main{flex:1;min-height:0;display:flex}.demo-editor{flex:1;min-width:0;display:flex;flex-direction:column;position:relative}.demo-canvas{flex:1;min-height:0;position:relative;overflow:hidden}
.demo-workspace{--demo-side-panel-width:240px;--demo-left-panel-width:0px;--demo-right-panel-width:0px;--dmn-grid-controls-left:var(--demo-left-panel-width);flex:1;min-height:0;position:relative}.demo-workspace[data-canvas-open=true]{--demo-left-panel-width:var(--demo-side-panel-width)}.demo-canvas-panel{left:0;right:auto;width:var(--demo-side-panel-width);box-shadow:var(--ui-shadow-panel-left)}.demo-canvas-panel .dmn-panel-header{padding-left:48px}
.demo-workspace[data-panel-open=true]{--demo-right-panel-width:var(--demo-side-panel-width)}.demo-workspace>[data-dmn-panel-frame]{width:var(--demo-side-panel-width)}.demo-center{height:100%;min-width:0;display:flex;flex-direction:column}.demo-center>.demo-preview-dock{margin-left:var(--demo-left-panel-width);margin-right:var(--demo-right-panel-width)}
`;
document.head.append(style);
if (role === 'overlay') document.body.style.background = 'transparent';

export const Main = () => {
  runtime.useAppBootstrap();
  const bootstrapped = runtime.useKeyStore((s) => s.isBootstrapped);
  runtime.useCustomJsInjection(bootstrapped);
  runtime.useCustomCssInjection({
    scopeSelector: runtime.USER_CSS_SCOPE_SELECTOR,
  });
  runtime.usePluginDisplayElementsResponder();
  const keys = ui.useKeyManager();
  const { handleUndo, handleRedo } = keys;
  const palette = ui.usePalette();
  const { t } = ui.useTranslation();
  const dialogs = ui.useMainDialogRuntime({ t });
  ui.useMainDialogRuntimeLifecycle(dialogs);
  const [activeTool, setActiveTool] = useState('select');
  const [addRequest, setAddRequest] = useState<{
    id: number;
    type: 'key' | 'stat' | 'graph' | 'knob' | 'sprite';
  } | null>(null);
  const [noteOpen, setNoteOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [preview, setPreview] = useState(true);
  const [canvasOpen, setCanvasOpen] = useState(true);
  const sidePanelOpen = runtime.usePropertiesPanelStore(
    (state) => state.isCanvasPanelOpen || !!state.pluginSettingsPanel,
  );
  const primaryButtonRef = useRef<HTMLButtonElement>(null!);
  const noteSettings = runtime.useSettingsStore((s) => s.noteSettings);
  const notePresence = ui.useModalPresence(noteOpen);
  const shownNoteSettings = ui.useRetainedWhileOpen(noteOpen, noteSettings);
  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      const target = event.target;
      if (
        target instanceof HTMLElement &&
        target.closest(
          'input,textarea,[contenteditable="true"],[role="dialog"]',
        )
      )
        return;
      if (!(event.metaKey || event.ctrlKey) || event.code !== 'KeyZ') return;
      event.preventDefault();
      if (event.shiftKey) handleRedo();
      else handleUndo();
    };
    window.addEventListener('keydown', listener);
    return () => window.removeEventListener('keydown', listener);
  }, [handleUndo, handleRedo]);
  return (
    <div className="demo-shell">
      <div className="demo-main">
        <div className="demo-editor">
          <div
            className="demo-workspace"
            data-panel-open={sidePanelOpen}
            data-canvas-open={canvasOpen}
          >
            <aside
              id="demo-canvas-panel"
              className={`${ui.SIDE_PANEL_FRAME_CLASS} demo-canvas-panel`}
              hidden={!canvasOpen}
              aria-label="캔버스 패널"
            >
              <ui.CanvasPanel
                onSwitchToProperty={() => {
                  const state = runtime.usePropertiesPanelStore.getState();
                  state.setCanvasPanelMode('property');
                  state.setCanvasPanelOpen(true);
                }}
              />
            </aside>
            <ui.SidePanelToggle
              side="left"
              open={canvasOpen}
              onClick={() => setCanvasOpen((open) => !open)}
              ariaLabel={canvasOpen ? '캔버스 패널 닫기' : '캔버스 패널 열기'}
              ariaControls="demo-canvas-panel"
            />
            <div className="demo-center">
              <div
                className="demo-canvas"
                onMouseEnter={() =>
                  runtime.useUIStore.getState().setGridAreaHovered(true)
                }
                onMouseLeave={() =>
                  runtime.useUIStore.getState().setGridAreaHovered(false)
                }
              >
                {bootstrapped && (
                  <ui.Grid
                    keyMappings={keys.keyMappings}
                    positions={keys.positions}
                    color={palette.color}
                    activeTool={activeTool}
                    showConfirm={dialogs.showConfirm}
                    showAlert={dialogs.showAlert}
                    onUndo={keys.handleUndo}
                    onRedo={keys.handleRedo}
                    toolbarAddRequest={addRequest}
                    onToolbarAddConsumed={() => setAddRequest(null)}
                    isNoteSettingOpen={noteOpen}
                    setIsNoteSettingOpen={setNoteOpen}
                  />
                )}
              </div>
              <WebDemoPreview
                documentId={documentId}
                open={preview}
                onOpenChange={setPreview}
              />
            </div>
            <ui.PropertiesPanel
              includeCanvasPanel={false}
              visibilityMode="manual"
              onKeyMappingChange={keys.handleKeyMappingChange}
            />
          </div>
          <ui.ToolBar
            onAddItem={(type) => setAddRequest({ id: Date.now(), type })}
            onTogglePalette={() => palette.setPalette(!palette.palette)}
            onClosePalette={palette.handlePaletteClose}
            isPaletteOpen={palette.palette}
            onResetCurrentMode={() =>
              dialogs.showConfirm(
                '현재 탭을 초기화할까요?',
                keys.handleResetCurrentMode,
              )
            }
            activeTool={activeTool}
            setActiveTool={setActiveTool}
            showAlert={dialogs.showAlert}
            onOpenNoteSetting={() => setNoteOpen(true)}
            onOpenSettings={() => setSettingsOpen(true)}
            primaryButtonRef={primaryButtonRef}
          />
        </div>
      </div>
      <ui.EditorSaveNotice />
      <ui.EditorSettingsModal
        isOpen={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        showAlert={dialogs.showAlert}
      />
      <ui.FloatingPopup
        open={palette.palette}
        ariaLabel="배경색"
        referenceRef={primaryButtonRef}
        placement="top"
        offset={25}
        onClose={palette.handlePaletteClose}
        className={ui.CANVAS_POPUP_CHROME_CLASS}
      >
        <ui.Palette
          color={palette.color}
          onColorChange={(value) => {
            if (typeof value === 'string') palette.handleColorChange(value);
          }}
        />
      </ui.FloatingPopup>
      {notePresence.mounted && shownNoteSettings && (
        <ui.NoteSettingModal
          key={notePresence.cycle}
          motionState={notePresence.state}
          settings={shownNoteSettings}
          onClose={() => setNoteOpen(false)}
          onSave={async (value) => {
            await runtime.internalApi.settings.update({ noteSettings: value });
          }}
        />
      )}
      <ui.CustomAlert
        {...dialogs.alertState}
        onConfirm={dialogs.handleAlertConfirm}
        onCancel={dialogs.handleAlertCancel}
      />
      <ui.CustomAlert
        isOpen={dialogs.customDialogState.isOpen}
        message={dialogs.customDialogState.html}
        type="custom"
        confirmText={dialogs.customDialogState.confirmText}
        cancelText={dialogs.customDialogState.cancelText}
        showCancel={dialogs.customDialogState.showCancel}
        onCustomContentMount={dialogs.customDialogState.onContentMount}
        onConfirm={dialogs.handleCustomDialogConfirm}
        onCancel={dialogs.handleCustomDialogCancel}
      />
      <ui.PopupExit open={dialogs.colorPickerState.isOpen}>
        {dialogs.colorPickerState.isOpen ? (
          <ui.ColorPicker
            open
            color={dialogs.colorPickerState.color}
            onColorChange={(value) => {
              if (typeof value === 'string')
                dialogs.handleGlobalColorChange(value);
            }}
            onColorChangeComplete={(value) => {
              if (typeof value === 'string')
                dialogs.handleGlobalColorChangeComplete(value);
            }}
            onClose={dialogs.closeColorPicker}
            position={dialogs.colorPickerState.position}
            referenceRef={{
              current:
                dialogs.colorPickerState.referenceElement ?? document.body,
            }}
            offsetY={dialogs.colorPickerState.referenceElement ? 10 : -80}
            placement="right"
            solidOnly
            closeOnScroll
          />
        ) : null}
      </ui.PopupExit>
    </div>
  );
};

const Surface =
  role === 'main' ? Main : (await import('./web-demo-overlay')).default;
createRoot(document.getElementById('root')!).render(
  <ui.I18nProvider>
    <Surface />
  </ui.I18nProvider>,
);
