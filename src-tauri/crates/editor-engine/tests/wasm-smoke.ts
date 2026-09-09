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
