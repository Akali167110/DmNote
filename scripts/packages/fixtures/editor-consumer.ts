import type { createIpcShim } from '@dmnote/ipc-shim';
import type {
  CanonicalEditorDocumentV1,
  EditorCommittedV1,
  EditorCoordinatorTransport,
} from '@dmnote/editor/model';
import type * as EditorExports from '@dmnote/editor/editor';
import type {} from '@dmnote/editor/globals';
import type * as PluginExports from '@dmnote/editor/plugins';
import '@dmnote/editor/style.css';

// 모든 공개 진입점 선언을 외부 소비자 관점에서 검사
export type PackageSurface = typeof EditorExports & typeof PluginExports;

const assert = (condition: unknown, message: string): void => {
  if (!condition) throw new Error(message);
};

async function verify() {
  const { shim, unexpectedCommands } = (
    window as unknown as {
      __EDITOR_PACKAGE_HOST__: {
        shim: ReturnType<typeof createIpcShim>;
        unexpectedCommands: string[];
      };
    }
  ).__EDITOR_PACKAGE_HOST__;
  // API 전역과 Tauri 창 정보가 생성되기 전에 호스트 설치
  await import('@dmnote/editor/install');
  const { OverlayScene } = await import('@dmnote/editor/overlay');
  const { Grid, PropertiesPanel } = await import('@dmnote/editor/editor');
  const [{ default: NarrowGrid }, toolbar, panelHost, dialogs] =
    await Promise.all([
      import('@dmnote/editor/grid'),
      import('@dmnote/editor/toolbar'),
      import('@dmnote/editor/panel-host'),
      import('@dmnote/editor/dialogs'),
    ]);
  assert(
    NarrowGrid === Grid &&
      [toolbar.default, panelHost.default, dialogs.useMainDialogRuntime].every(
        (entry) => entry !== undefined,
      ),
    'Narrow entrypoint contract mismatch',
  );
  assert(
    window.api.window.type === 'overlay',
    'Installed host role was not preserved',
  );
  const { createCustomJsRuntime } = await import('@dmnote/editor/plugins');
  assert(
    [Grid, PropertiesPanel, createCustomJsRuntime].every(
      (entry) => entry !== undefined,
    ),
    'Missing public runtime exports',
  );
  const { useSettingsStore } = await import('@dmnote/editor/runtime');
  const {
    createEditorCoordinator,
    createDefaultKeyPosition,
    applyEditorPatch,
    createEditorPatch,
    getChangedEditorFields,
  } = await import('@dmnote/editor/model');
  const { createElement } = await import('react');
  const { createRoot } = await import('react-dom/client');
  const { flushSync } = await import('react-dom');
  const container = document.getElementById('root');
  if (!container) throw new Error('Missing fixture container');
  const root = createRoot(container);
  let canonical: CanonicalEditorDocumentV1 = {
    schemaVersion: 1,
    keys: { '4key': ['A'] },
    keyPositions: {
      '4key': [
        {
          ...createDefaultKeyPosition(10, 20),
          id: '11111111-1111-4111-8111-111111111111',
        },
      ],
    },
    statPositions: {},
    graphPositions: {},
    knobPositions: {},
    spritePositions: {},
    layerGroups: {},
  };
  let local = structuredClone(canonical);
  let revision = 0;
  let commitCount = 0;
  let listener: ((event: EditorCommittedV1) => void) | null = null;
  let committedEventsApplied = 0;
  const appliedReasons: string[] = [];

  const render = () => {
    flushSync(() =>
      root.render(
        createElement(OverlayScene, {
          currentKeys: local.keys['4key'].map((slot) =>
            typeof slot === 'string' ? slot : slot.keys[0],
          ),
          currentKeyLabels: local.keys['4key'].map((slot) =>
            typeof slot === 'string' ? slot : slot.keys.join('+'),
          ),
          displayPositions: local.keyPositions['4key'],
          currentPositions: local.keyPositions['4key'],
          displayStatPositions: [],
          displayGraphPositions: [],
          displayKnobPositions: [],
          selectedKeyType: '4key',
          noteEffect: false,
          noteSettings: useSettingsStore.getState().noteSettings,
          webglTracks: [],
          notesRef: { current: [] },
          subscribe: () => () => {},
          noteBuffer: null,
          backgroundColor: 'transparent',
          keyCounterEnabled: false,
          showPluginElements: false,
        }),
      ),
    );
  };
  const transport: EditorCoordinatorTransport = {
    get: async () => ({ revision, document: structuredClone(canonical) }),
    commit: async (request) => {
      assert(
        request.baseRevision === revision,
        'Coordinator sent stale revision',
      );
      if (!request.changes)
        throw new Error('Fixture only handles patch commits');
      const next = applyEditorPatch(canonical, request.changes);
      const changedFields = getChangedEditorFields(canonical, next);
      canonical = next;
      revision += 1;
      commitCount += 1;
      return { revision, changedFields };
    },
    onCommitted: (callback) => {
      listener = callback;
      return Object.assign(
        () => {
          listener = null;
        },
        { ready: Promise.resolve() },
      );
    },
  };
  const coordinator = createEditorCoordinator({
    transport,
    readDocument: () => local,
    applyDocument: (next, reason) => {
      local = next;
      appliedReasons.push(reason);
      render();
    },
    createMutationId: () => '22222222-2222-4222-8222-222222222222',
    focusTarget: null,
    visibilityTarget: null,
    onCommittedApplied: () => {
      committedEventsApplied += 1;
    },
  });

  try {
    await coordinator.start();
    assert(
      container.textContent?.includes('A'),
      'Initial model did not render key A',
    );
    assert(
      container.querySelector('.dmn-overlay-background'),
      'Actual OverlayScene background missing',
    );
    await coordinator.commitPatch({
      schemaVersion: 2,
      keys: { '4key': ['B'] },
    });
    assert(
      commitCount === 1 && revision === 1,
      'Expected one confirmed transport commit',
    );
    assert(!coordinator.getState().dirty, 'Confirmed edit remained dirty');
    assert(
      container.textContent?.includes('B'),
      'Committed key label not reflected in DOM',
    );

    const before = canonical;
    canonical = { ...canonical, keys: { '4key': ['C'] } };
    revision += 1;
    if (!listener)
      throw new Error('Coordinator did not register committed listener');
    (listener as (event: EditorCommittedV1) => void)({
      schemaVersion: 1,
      mutationId: '33333333-3333-4333-8333-333333333333',
      revision,
      changedFields: getChangedEditorFields(before, canonical),
      patch: createEditorPatch(before, canonical),
    });
    await coordinator.flush();
    assert(
      committedEventsApplied === 1,
      'Committed event was not applied exactly once',
    );
    assert(
      coordinator.getState().revision === 2,
      'Committed event revision not adopted',
    );
    assert(
      container.textContent?.includes('C'),
      'Committed event did not update actual key DOM',
    );
    assert(
      appliedReasons.includes('event'),
      'Event apply path was not exercised',
    );
    assert(
      unexpectedCommands.length === 0,
      `Unexpected native dependencies: ${unexpectedCommands.join(', ')}`,
    );
    return {
      commits: commitCount,
      revision,
      labels: ['A', 'B', 'C'],
      appliedReasons,
    };
  } finally {
    coordinator.stop();
    flushSync(() => root.unmount());
    shim.dispose();
  }
}

Object.assign(window, { __EDITOR_PACKAGE_CHECK__: verify() });
