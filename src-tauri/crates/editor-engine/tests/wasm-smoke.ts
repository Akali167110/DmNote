import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { resolve } from 'node:path';

const require = createRequire(import.meta.url);
const engine = require(resolve(process.argv[2]));
const initial = JSON.parse(engine.migrate_store_json('{}', 0n)).store;
const session = new engine.EditorSession(JSON.stringify(initial));
const before = JSON.parse(session.snapshot());
const positions = structuredClone(initial.keyPositions);
positions['4key'][0].dx += 7;
const request = {
  baseRevision: initial.editorRevision,
  mutationId: '5717a6e5-759e-4739-a6d3-14e853c66e23',
  changes: { schemaVersion: 1, keyPositions: positions },
};
const prepared = JSON.parse(session.prepare(JSON.stringify(request)));
assert.equal(prepared.store.editorRevision, initial.editorRevision + 1);
assert.deepEqual(JSON.parse(session.snapshot()), before);
assert.throws(
  () => session.prepare(JSON.stringify(request)),
  /EDITOR_SAVE_PENDING/,
);
assert.throws(
  () => session.confirm('another-mutation'),
  /EDITOR_PENDING_MUTATION_MISMATCH/,
);
session.discard(request.mutationId);
assert.deepEqual(JSON.parse(session.snapshot()), before);
session.prepare(JSON.stringify(request));
const confirmed = JSON.parse(session.confirm(request.mutationId));
assert.equal(confirmed.result.revision, initial.editorRevision + 1);
assert.equal(
  confirmed.event.patch.keyPositions['4key'][0].dx,
  positions['4key'][0].dx,
);
assert.equal(
  JSON.parse(session.snapshot()).store.keyPositions['4key'][0].dx,
  positions['4key'][0].dx,
);
const replay = JSON.parse(session.prepare(JSON.stringify(request)));
assert.equal(replay.replayed, true);
assert.deepEqual(replay.result, confirmed.result);
const reused = structuredClone(request);
reused.changes.keyPositions['4key'][0].dx += 1;
assert.throws(
  () => session.prepare(JSON.stringify(reused)),
  /MUTATION_ID_REUSED/,
);
assert.throws(
  () =>
    session.prepare(
      JSON.stringify({
        ...request,
        mutationId: '85a950e0-4d2d-4750-a80d-95b1a075954a',
      }),
    ),
  /REVISION_CONFLICT/,
);
assert.equal(
  JSON.parse(engine.decode_preset_json('{"keys":{"4key":["A"]}}')).keys[
    '4key'
  ][0],
  'A',
);
assert.throws(() => engine.decode_preset_json('not json'), /invalid-preset/);
session.free();
console.log(
  'WASM editor: prepare/discard/confirm, mutation replay, conflicts, migration and preset parsing passed.',
);

const web = new engine.WebEditorSession(JSON.stringify(initial));
assert.equal(engine.web_command_kind('editor_commit'), 'write');
assert.equal(engine.web_command_kind('sound_list'), 'write');
assert.equal(engine.web_command_kind('app_bootstrap'), 'read');
assert.equal(engine.web_command_kind('app_quit'), 'unsupported');
const webBefore = web.snapshot();
const webPrepared = web.prepare_command(
  'editor_commit',
  JSON.stringify({ request }),
  'save-1',
);
assert.equal(web.snapshot(), webBefore);
web.discard_command('save-1');
assert.equal(web.snapshot(), webBefore);
assert.equal(
  web.prepare_command('editor_commit', JSON.stringify({ request }), 'save-2'),
  web.confirm_command('save-2'),
);
const undoArgs = JSON.stringify({
  operationId: 'e7d00c5f-a90c-48ea-a479-58723f19062b',
});
const undo = web.prepare_command('history_undo', undoArgs, 'undo');
assert.equal(undo, web.confirm_command('undo'));
assert.equal(
  JSON.parse(web.read('editor_get', '{}')).document.keyPositions['4key'][0].dx,
  initial.keyPositions['4key'][0].dx,
);
const authorityPrepared = web.prepare_command(
  'plugin_authority_reset',
  '{}',
  'authority',
);
const authority = JSON.parse(web.confirm_command('authority'));
assert.equal(authorityPrepared, JSON.stringify(authority));
assert.equal(authority.result.authorityGeneration, 1);
const keyboard = new engine.WebKeyboardMatcher(
  JSON.stringify({ '4key': [{ keys: ['A', 'B'], match: 'all' }] }),
  '4key',
);
const feed = (key: string, isDown: boolean) =>
  JSON.parse(
    keyboard.feed(
      JSON.stringify({
        physicalId: key,
        device: 'keyboard',
        candidates: [key],
        isDown,
      }),
    ),
  );
assert.equal(feed('A', true).events[0].transition, null);
assert.equal(feed('A', true), null);
assert.equal(feed('B', true).events[0].transition, true);
assert.equal(feed('A', false).events[0].transition, false);
const preview = {
  schemaVersion: 1,
  sessionId: '201a39be-e259-46d4-81c2-9e43e5111dc2',
  seq: 1,
  domain: 'keyPosition',
  mode: '4key',
  targets: [0],
  patch: { dx: 12 },
};
assert.equal(
  JSON.parse(engine.validate_preview_json(JSON.stringify(preview), 'main'))
    .sourceLabel,
  'main',
);
assert.equal(engine.is_preview_session_id(preview.sessionId), true);
assert.equal(engine.is_preview_session_id('invalid'), false);
assert.throws(
  () =>
    engine.validate_preview_json(
      JSON.stringify({ ...preview, patch: { arbitrary: 1 } }),
      'main',
    ),
  /not allowed/,
);
assert.throws(
  () =>
    engine.validate_preview_json(
      JSON.stringify({
        ...preview,
        patch: { backgroundGradient: { angle: 0, stops: [] } },
      }),
      'main',
    ),
  /stops/,
);
const checkpoint = JSON.parse(web.snapshot());
const restarted = new engine.WebEditorSession(JSON.stringify(checkpoint.store));
restarted.restore_checkpoint(JSON.stringify(checkpoint.checkpoint));
assert.equal(JSON.parse(restarted.snapshot()).authorityGeneration, 1);
assert.equal(JSON.parse(restarted.read('history_status', '{}')).canUndo, false);
assert.throws(
  () => restarted.restore_checkpoint(JSON.stringify(checkpoint.checkpoint)),
  /ALREADY_INITIALIZED/,
);
const disconnected = JSON.parse(
  restarted.prepare_command('web_main_disconnected', '{}', 'main-close'),
);
assert.equal(disconnected.store, null);
assert.equal(disconnected.checkpoint.authorityAvailable, false);
restarted.confirm_command('main-close');
assert.throws(
  () =>
    restarted.prepare_command(
      'plugin_instances_commit',
      JSON.stringify({
        request: {
          pluginId: 'closed',
          instances: [],
          mutationId: '7672d0a1-886b-4f53-ab0b-e55d9dc21cf3',
          authorityGeneration: 1,
        },
      }),
      'stale',
    ),
  /AUTHORITY_UNAVAILABLE/,
);
restarted.free();
keyboard.free();
web.free();
console.log(
  'WASM web host: atomic command sessions, undo, authority, keyboard matcher and preview validation passed.',
);
