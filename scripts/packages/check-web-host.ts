import { execFileSync } from 'node:child_process';
import {
  mkdtemp,
  writeFile,
  copyFile,
  cp,
  readFile,
  rm,
  realpath,
} from 'node:fs/promises';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import os from 'node:os';
import assert from 'node:assert/strict';
import { build } from 'vite';
import { chromium, type Page } from 'playwright-core';
import ts from 'typescript';
import type { WebConsumerTest } from './fixtures/web-consumer';

const root = fileURLToPath(new URL('../../', import.meta.url));
const temp = await realpath(
  await mkdtemp(path.join(os.tmpdir(), 'dmnote-web-consumer-')),
);
const npmCli = process.env.npm_execpath;
const runNpm = (args: string[], cwd: string) =>
  execFileSync(
    npmCli ? process.execPath : 'npm',
    npmCli ? [npmCli, ...args] : args,
    { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] },
  );
let browser: Awaited<ReturnType<typeof chromium.launch>> | undefined;
let server: ReturnType<typeof createServer> | undefined;
const state = (page: Page) =>
  page.evaluate(() => ({
    events: (window as unknown as { webTest: WebConsumerTest }).webTest.events,
    previews: (window as unknown as { webTest: WebConsumerTest }).webTest
      .previews,
    downloads: (window as unknown as { webTest: WebConsumerTest }).webTest
      .downloads,
  }));
const invoke = <T = unknown>(
  page: Page,
  command: string,
  args: Record<string, unknown> = {},
) =>
  page.evaluate(
    ({ command, args }) =>
      (window as unknown as { webTest: WebConsumerTest }).webTest.host
        .invoke<T>(command, args)
        .catch((error) => {
          throw new Error(JSON.stringify({ command, error }));
        }),
    { command, args },
  );
try {
  const dependencies: Record<string, string> = {
    react: '^19.0.0',
    'react-dom': '^19.0.0',
    '@tauri-apps/api': '~2.11.0',
    '@types/react': '^19.0.0',
    '@types/react-dom': '^19.0.0',
  };
  for (const name of ['editor', 'ipc-shim', 'web-host']) {
    const packed = JSON.parse(
      runNpm(
        ['pack', '--json', '--pack-destination', temp],
        path.join(root, 'packages', name),
      ),
    ) as Array<{ filename: string }>;
    dependencies[`@dmnote/${name}`] = `file:${path.join(
      temp,
      packed[0].filename,
    )}`;
  }
  await writeFile(
    path.join(temp, 'package.json'),
    JSON.stringify({
      name: 'web-consumer-check',
      private: true,
      type: 'module',
      dependencies,
    }),
  );
  runNpm(['install', '--ignore-scripts', '--no-audit', '--no-fund'], temp);
  await copyFile(
    path.join(root, 'scripts/packages/fixtures/web-consumer.ts'),
    path.join(temp, 'main.ts'),
  );
  const program = ts.createProgram([path.join(temp, 'main.ts')], {
    noEmit: true,
    strict: true,
    skipLibCheck: false,
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    lib: ['lib.es2022.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'],
    types: ['react', 'react-dom'],
    typeRoots: [path.join(temp, 'node_modules/@types')],
  });
  const diagnostics = ts.getPreEmitDiagnostics(program);
  if (diagnostics.length)
    throw new Error(
      ts.formatDiagnosticsWithColorAndContext(diagnostics, {
        getCanonicalFileName: (name) => name,
        getCurrentDirectory: () => temp,
        getNewLine: () => '\n',
      }),
    );
  await writeFile(
    path.join(temp, 'index.html'),
    '<html><head></head><body><div id="root"></div><script type="module" src="/main.ts"></script></body></html>',
  );
  await build({
    configFile: false,
    root: temp,
    logLevel: 'warn',
    build: { target: 'es2022', outDir: 'dist' },
  });
  await cp(
    path.join(temp, 'node_modules/@dmnote/web-host/dist'),
    path.join(temp, 'dist/engine'),
    { recursive: true },
  );
  server = createServer((request, response) => {
    void (async () => {
      const requested = new URL(request.url ?? '/', 'http://localhost')
        .pathname;
      const relative = requested === '/' ? 'index.html' : requested.slice(1);
      const file = path.resolve(temp, 'dist', relative);
      if (!file.startsWith(path.join(temp, 'dist') + path.sep))
        throw new Error('Invalid path');
      const type = file.endsWith('.wasm')
        ? 'application/wasm'
        : file.endsWith('.js')
        ? 'text/javascript'
        : file.endsWith('.css')
        ? 'text/css'
        : 'text/html';
      response.setHeader('Content-Type', type);
      response.end(await readFile(file));
    })().catch(() => {
      response.statusCode = 404;
      response.end();
    });
  });
  await new Promise<void>((resolve) => server!.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  if (!address || typeof address === 'string')
    throw new Error('Missing test server address');
  const origin = `http://127.0.0.1:${address.port}`;
  browser = await chromium.launch({
    headless: true,
    ...(process.env.DMNOTE_BROWSER_PATH
      ? { executablePath: process.env.DMNOTE_BROWSER_PATH }
      : { channel: 'chrome' }),
  });
  const context = await browser.newContext();
  const failures: string[] = [];
  const open = async (documentId: string, role: 'main' | 'overlay') => {
    const page = await context.newPage();
    page.on('pageerror', (error) => failures.push(error.message));
    await page.goto(`${origin}/?document=${documentId}&role=${role}`);
    await page.waitForFunction(() =>
      Boolean((window as unknown as { ready?: boolean }).ready),
    );
    return page;
  };
  const main = await open('shared', 'main');
  const overlay = await open('shared', 'overlay');
  const isolated = await open('isolated', 'main');
  const bootstrap = await invoke<{
    currentMode: string;
    keys: Record<string, unknown[]>;
  }>(main, 'app_bootstrap');
  assert.equal(bootstrap.currentMode, '4key');
  assert.ok(bootstrap.keys['4key'].length);
  type Document = {
    revision: number;
    document: { keyPositions: Record<string, Array<Record<string, unknown>>> };
  };
  const before = await invoke<Document>(main, 'editor_get');
  const positions = structuredClone(before.document.keyPositions);
  positions['4key'][0].dx = 123;
  const mutationId = crypto.randomUUID();
  const request = {
    mutationId,
    baseRevision: before.revision,
    changes: { schemaVersion: 1, keyPositions: positions },
    multiKey: true,
  };
  const committed = await invoke<{ revision: number }>(main, 'editor_commit', {
    request,
  });
  assert.equal(committed.revision, before.revision + 1);
  assert.equal(
    (await invoke<Document>(overlay, 'editor_get')).document.keyPositions[
      '4key'
    ][0].dx,
    123,
  );
  assert.notEqual(
    (await invoke<Document>(isolated, 'editor_get')).document.keyPositions[
      '4key'
    ][0].dx,
    123,
  );
  await invoke(main, 'plugin_bridge_send_to', {
    target: 'overlay',
    messageType: 'integration',
    data: { value: 7 },
  });
  assert.ok(
    (await state(overlay)).events.some(
      (event) =>
        event.event === 'plugin-bridge:message' &&
        (event.payload as { type: string }).type === 'integration',
    ),
  );
  const gestureId = crypto.randomUUID();
  await invoke(main, 'editor_preview_publish', {
    request: {
      schemaVersion: 1,
      sessionId: gestureId,
      seq: 1,
      domain: 'keyPosition',
      mode: '4key',
      targets: [0],
      patch: { dx: 456 },
    },
  });
  await overlay.waitForFunction(
    () =>
      (window as unknown as { webTest: WebConsumerTest }).webTest.previews
        .length > 0,
  );
  assert.equal(
    ((await state(overlay)).previews[0] as { sourceLabel: string }).sourceLabel,
    'main',
  );
  await invoke(main, 'editor_preview_cancel', { sessionId: gestureId });
  await invoke(main, 'history_undo', { operationId: crypto.randomUUID() });
  assert.deepEqual(
    (await invoke<Document>(main, 'editor_get')).document.keyPositions,
    before.document.keyPositions,
  );
  await invoke(main, 'history_redo', { operationId: crypto.randomUUID() });
  assert.equal(
    (await invoke<Document>(main, 'editor_get')).document.keyPositions[
      '4key'
    ][0].dx,
    123,
  );
  await invoke(main, 'plugin_storage_set', {
    key: 'integration',
    value: { saved: 9 },
  });
  assert.deepEqual(
    await invoke(overlay, 'plugin_storage_get', { key: 'integration' }),
    { saved: 9 },
  );
  assert.equal(
    await invoke(isolated, 'plugin_storage_get', { key: 'integration' }),
    null,
  );
  const png =
    'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aG1sAAAAASUVORK5CYII=';
  await main.evaluate((dataBase64) => {
    (window as unknown as { webTest: WebConsumerTest }).webTest.files.push({
      name: 'pixel.png',
      mimeType: 'image/png',
      bytes: Uint8Array.from(atob(dataBase64), (character) =>
        character.charCodeAt(0),
      ).buffer,
    });
  }, png);
  const image = await invoke<{ success: boolean; imagePath: string }>(
    main,
    'image_load',
  );
  assert.equal(image.success, true);
  const withAsset = await invoke<Document>(main, 'editor_get');
  withAsset.document.keyPositions['4key'][0].inactiveImage = image.imagePath;
  await invoke(main, 'editor_commit', {
    request: {
      mutationId: crypto.randomUUID(),
      baseRevision: withAsset.revision,
      multiKey: true,
      changes: {
        schemaVersion: 1,
        keyPositions: withAsset.document.keyPositions,
      },
    },
  });
  await invoke(main, 'preset_save');
  const exported = (await state(main)).downloads.at(-1);
  assert.ok(exported?.name.endsWith('.json'));
  assert.ok(JSON.parse(exported!.content));
  assert.ok(exported!.content.includes(png));
  await isolated.evaluate((content) => {
    (window as unknown as { webTest: WebConsumerTest }).webTest.files.push({
      name: 'import.json',
      mimeType: 'application/json',
      bytes: new TextEncoder().encode(content).buffer,
    });
  }, exported!.content);
  await invoke(isolated, 'preset_load');
  const imported = await invoke<Document>(isolated, 'editor_get');
  const importedImage = String(
    imported.document.keyPositions['4key'][0].inactiveImage,
  );
  assert.ok(importedImage.startsWith('/assets/images/'));
  const actualImage = await isolated.evaluate(async (key) => {
    const url = (
      window as unknown as { webTest: WebConsumerTest }
    ).webTest.host.assets.resolve(key);
    return btoa(
      String.fromCharCode(
        ...new Uint8Array(await (await fetch(url)).arrayBuffer()),
      ),
    );
  }, importedImage);
  assert.equal(actualImage, png);
  assert.equal(
    await invoke(isolated, 'plugin_storage_get', { key: 'integration' }),
    null,
  );
  await invoke(main, 'raw_input_subscribe');
  await main.keyboard.press('KeyD');
  await main.waitForFunction(() =>
    (window as unknown as { webTest: WebConsumerTest }).webTest.events.some(
      (event) => event.event === 'keys:state',
    ),
  );
  await main.evaluate(() =>
    (
      window as unknown as { webTest: WebConsumerTest }
    ).webTest.host.reconnect(),
  );
  assert.equal(
    (await invoke<Document>(main, 'editor_get')).document.keyPositions[
      '4key'
    ][0].dx,
    123,
  );
  await main.evaluate(() =>
    (window as unknown as { webTest: WebConsumerTest }).webTest.mount(),
  );
  await main.waitForSelector('[data-bootstrapped="true"]');
  await main.waitForSelector('[data-grid-container]');
  await invoke(main, 'css_set_content', {
    content: '[data-web-css-test] { color: rgb(12, 34, 56); }',
  });
  await invoke(main, 'css_toggle', { enabled: true });
  await main.evaluate(() => {
    const marker = document.createElement('span');
    marker.setAttribute('data-web-css-test', '');
    marker.textContent = 'CSS';
    document.querySelector('[data-dmn-user-css-scope]')!.append(marker);
  });
  await main.waitForFunction(
    () =>
      getComputedStyle(document.querySelector('[data-web-css-test]')!).color ===
      'rgb(12, 34, 56)',
  );
  await invoke(main, 'js_set_content', {
    content: "document.body.dataset.webPlugin = 'executed';",
  });
  await invoke(main, 'js_toggle', { enabled: true });
  await main.waitForFunction(
    () => document.body.dataset.webPlugin === 'executed',
  );
  assert.deepEqual(failures, []);
  await main.close();
  await overlay.close();
  const reopened = await open('shared', 'main');
  assert.equal(
    (await invoke<Document>(reopened, 'editor_get')).document.keyPositions[
      '4key'
    ][0].dx,
    123,
  );
  assert.deepEqual(
    await invoke(reopened, 'plugin_storage_get', { key: 'integration' }),
    { saved: 9 },
  );
  await isolated.evaluate(async () => {
    const db = await new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open('dmnote-web-editor-v1', 1);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    try {
      await new Promise<void>((resolve, reject) => {
        const tx = db.transaction('documents', 'readwrite');
        const docs = tx.objectStore('documents');
        const request = docs.get('shared');
        request.onsuccess = () => {
          const record = request.result as {
            store: Record<string, unknown>;
            checkpoint: Record<string, unknown>;
          };
          docs.put({
            id: 'corrupt-document',
            version: 1,
            incarnation: 'original',
            store: { ...record.store, editorRevision: 'damaged' },
            checkpoint: record.checkpoint,
          });
          docs.put({
            id: 'corrupt-checkpoint',
            version: 1,
            incarnation: 'original',
            store: record.store,
            checkpoint: { invalid: true },
          });
        };
        tx.oncomplete = () => resolve();
        tx.onabort = () => reject(tx.error);
      });
    } finally {
      db.close();
    }
  });
  const recovered = await open('corrupt-document', 'main');
  assert.equal(
    (await invoke<Document>(recovered, 'editor_get')).document.keyPositions[
      '4key'
    ][0].dx,
    123,
  );
  const recoveredCheckpoint = await open('corrupt-checkpoint', 'main');
  assert.equal(
    (await invoke<Document>(recoveredCheckpoint, 'editor_get')).document
      .keyPositions['4key'][0].dx,
    123,
  );
  const recoveryRecords = await isolated.evaluate(async () => {
    const db = await new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open('dmnote-web-editor-v1', 1);
      request.onsuccess = () => resolve(request.result);
      request.onerror = () => reject(request.error);
    });
    try {
      return await Promise.all(
        ['corrupt-document', 'corrupt-checkpoint'].map(
          (id) =>
            new Promise<{
              incarnation: string;
              recoveryStore?: Record<string, unknown>;
              recoveryCheckpoint?: Record<string, unknown>;
            }>((resolve, reject) => {
              const request = db
                .transaction('documents')
                .objectStore('documents')
                .get(id);
              request.onsuccess = () => resolve(request.result);
              request.onerror = () => reject(request.error);
            }),
        ),
      );
    } finally {
      db.close();
    }
  });
  assert.equal(recoveryRecords[0].recoveryStore?.editorRevision, 'damaged');
  assert.deepEqual(recoveryRecords[1].recoveryCheckpoint, { invalid: true });
  assert.ok(
    recoveryRecords.every((record) => record.incarnation !== 'original'),
  );
  process.stdout.write(
    'External web-host packages: real Chromium SharedWorker + WASM + IndexedDB, document isolation, editor commit, preview Channel, history flush, plugin storage/bridge/DOM, asset preset roundtrip, keyboard input, reconnect, CSS, shared Grid mount and corrupt store/checkpoint recovery passed.\n',
  );
} finally {
  await browser?.close();
  await new Promise<void>((resolve) =>
    server ? server.close(() => resolve()) : resolve(),
  );
  if (process.env.DMNOTE_KEEP_WEB_CONSUMER === '1')
    process.stdout.write(`Web consumer retained: ${temp}\n`);
  else await rm(temp, { recursive: true, force: true });
}
