import { useEffect, useMemo } from 'react';
import {
  OverlayScene,
  computeLayout,
  useNoteSystem,
  useOverlayKeyStateRuntime,
  useBuiltinStatsSubscription,
} from '@dmnote/editor/overlay';
import * as runtime from '@dmnote/editor/runtime';

const EMPTY: never[] = [];

const Preview = () => {
  runtime.useAppBootstrap();
  const keys = runtime.useKeyStore();
  const settings = runtime.useSettingsStore();
  runtime.useCustomCssInjection();
  runtime.useCustomJsInjection(keys.isBootstrapped);
  useBuiltinStatsSubscription();
  const noteSettings = useMemo(
    () => ({
      ...settings.noteSettings,
      ...settings.tabNoteOverrides?.[keys.selectedKeyType],
    }),
    [settings.noteSettings, settings.tabNoteOverrides, keys.selectedKeyType],
  );
  const notes = useNoteSystem({
    noteEffect: settings.noteEffect,
    noteSettings,
  });
  const slots = keys.keyMappings[keys.selectedKeyType] ?? EMPTY;
  const active = useOverlayKeyStateRuntime({
    noteEffect: settings.noteEffect,
    keyDisplayDelayMs: noteSettings.keyDisplayDelayMs ?? 0,
    keyMappings: keys.keyMappings,
    currentSlots: slots,
    positions: keys.positions,
    selectedKeyType: keys.selectedKeyType,
    handleKeyDown: notes.handleKeyDown,
    handleKeyUp: notes.handleKeyUp,
    finalizeAllActive: notes.finalizeAllActive,
    reconcileActiveNotes: notes.reconcileActiveNotes,
  });
  const currentPositions = keys.positions[keys.selectedKeyType] ?? EMPTY;
  const stats = runtime.useStatItemStore(
    (s) => s.positions[keys.selectedKeyType] ?? EMPTY,
  );
  const graphs = runtime.useGraphItemStore(
    (s) => s.positions[keys.selectedKeyType] ?? EMPTY,
  );
  const knobs = runtime.useKnobItemStore(
    (s) => s.positions[keys.selectedKeyType] ?? EMPTY,
  );
  const sprites = runtime.useSpriteStore(
    (s) => s.positions[keys.selectedKeyType] ?? EMPTY,
  );
  const plugins = runtime.usePluginDisplayElementStore((s) => s.elements);
  const layout = useMemo(
    () =>
      computeLayout({
        currentKeys: active.currentKeys,
        currentPositions,
        currentStatPositions: stats,
        currentGraphPositions: graphs,
        currentKnobPositions: knobs,
        currentSpritePositions: sprites,
        trackHeight: settings.noteEffect ? noteSettings.trackHeight : 0,
        noteSettings,
        selectedKeyType: keys.selectedKeyType,
        pluginElements: plugins,
        overlayPadding: settings.gridSettings.overlayPadding ?? 30,
      }),
    [
      active.currentKeys,
      currentPositions,
      stats,
      graphs,
      knobs,
      sprites,
      settings.noteEffect,
      noteSettings,
      keys.selectedKeyType,
      plugins,
      settings.gridSettings.overlayPadding,
    ],
  );
  const { updateTrackLayouts } = notes;
  const contentWidth = layout.contentSize?.width ?? 640;
  const contentHeight = layout.contentSize?.height ?? 360;
  useEffect(() => {
    window.parent.postMessage(
      { type: 'demo:preview-size', width: contentWidth, height: contentHeight },
      location.origin,
    );
  }, [contentWidth, contentHeight]);
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !event.defaultPrevented) {
        window.parent.postMessage(
          { type: 'demo:preview-escape' },
          location.origin,
        );
      }
    };
    window.addEventListener('keydown', escape);
    return () => window.removeEventListener('keydown', escape);
  }, []);
  useEffect(
    () => updateTrackLayouts(layout.webglTracks),
    [layout.webglTracks, updateTrackLayouts],
  );
  const spriteKeyCanonicalMap = useMemo(
    () =>
      new Map(
        currentPositions.flatMap((position, index) =>
          position.id && active.currentKeys[index]
            ? [[position.id, active.currentKeys[index]] as const]
            : [],
        ),
      ),
    [currentPositions, active.currentKeys],
  );
  return (
    <OverlayScene
      {...active}
      displayPositions={layout.displayPositions}
      currentPositions={currentPositions}
      displayStatPositions={layout.displayStatPositions}
      displayGraphPositions={layout.displayGraphPositions}
      displayKnobPositions={layout.displayKnobPositions}
      displaySpritePositions={layout.displaySpritePositions}
      spriteKeyCanonicalMap={spriteKeyCanonicalMap}
      selectedKeyType={keys.selectedKeyType}
      noteEffect={settings.noteEffect}
      noteSettings={noteSettings}
      webglTracks={layout.webglTracks}
      notesRef={notes.notesRef}
      subscribe={notes.subscribe}
      noteBuffer={notes.noteBuffer}
      backgroundColor={settings.backgroundColor}
      keyCounterEnabled={settings.keyCounterEnabled}
      contentSize={layout.backgroundBox ?? undefined}
      positionOffset={layout.positionOffset}
      revealed={keys.isBootstrapped}
    />
  );
};

export default Preview;
