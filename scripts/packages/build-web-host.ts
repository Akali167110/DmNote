import { execFileSync } from 'node:child_process';
import { copyFile, mkdir, rm } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { build } from 'vite';

const root = fileURLToPath(new URL('../../', import.meta.url));
const output = path.join(root, 'packages/web-host/dist');
const run = (command: string, args: string[]) =>
  execFileSync(command, args, { cwd: root, stdio: 'inherit' });
await rm(output, { recursive: true, force: true });
await mkdir(output, { recursive: true });
await copyFile(path.join(root, 'LICENSE'), path.join(output, 'LICENSE'));
run('cargo', [
  'build',
  '--manifest-path',
  'src-tauri/Cargo.toml',
  '-p',
  'dmnote-editor-engine',
  '--target',
  'wasm32-unknown-unknown',
  '--release',
  '--locked',
]);
run('wasm-bindgen', [
  'src-tauri/target/wasm32-unknown-unknown/release/dmnote_editor_engine.wasm',
  '--target',
  'web',
  '--out-dir',
  path.join(output, 'wasm'),
  '--out-name',
  'dmnote_editor_engine',
]);
await build({
  configFile: false,
  build: {
    outDir: output,
    emptyOutDir: false,
    lib: {
      entry: path.join(root, 'packages/web-host/src/index.ts'),
      formats: ['es'],
      fileName: () => 'index.js',
    },
    rollupOptions: { external: (id) => id.startsWith('@dmnote/') },
  },
});
await build({
  configFile: false,
  build: {
    outDir: output,
    emptyOutDir: false,
    lib: {
      entry: path.join(root, 'packages/web-host/src/worker/index.ts'),
      formats: ['es'],
      fileName: () => 'worker.js',
    },
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
});
run(process.execPath, [
  'node_modules/typescript/bin/tsc',
  '-p',
  'packages/web-host/tsconfig.declarations.json',
]);
