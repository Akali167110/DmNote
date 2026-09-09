import { execFileSync } from 'node:child_process';
import { mkdtemp, readFile, writeFile, copyFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { MessageChannel } from 'node:worker_threads';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { build } from 'vite';
import { JSDOM, VirtualConsole } from 'jsdom';

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../..',
);
const temp = await mkdtemp(path.join(os.tmpdir(), 'dmnote-package-consumer-'));
const npmCli = process.env.npm_execpath;
if (process.platform === 'win32' && !npmCli) {
  throw new Error('Run this check with npm run check:packages on Windows');
}
const npm = npmCli ? process.execPath : 'npm';
const npmArgs = (args: string[]): string[] =>
  npmCli ? [npmCli, ...args] : args;
const keep = process.env.DMNOTE_KEEP_PACKAGE_CONSUMER === '1';

const pack = (directory: string): string => {
  const output = execFileSync(
    npm,
    npmArgs(['pack', '--json', '--pack-destination', temp]),
    {
      cwd: directory,
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'inherit'],
    },
  );
  const result = JSON.parse(output) as Array<{ filename: string }>;
  if (!result[0]?.filename)
    throw new Error(`npm pack produced no tarball: ${directory}`);
  return `file:${path.join(temp, result[0].filename)}`;
};

try {
  const editor = pack(path.join(root, 'packages/editor'));
  const shim = pack(path.join(root, 'packages/ipc-shim'));
  await writeFile(
    path.join(temp, 'package.json'),
    JSON.stringify(
      {
        name: 'dmnote-external-consumer-check',
        private: true,
        type: 'module',
        dependencies: {
          '@dmnote/editor': editor,
          '@dmnote/ipc-shim': shim,
          react: '^19.0.0',
          'react-dom': '^19.0.0',
          '@types/react': '^19.0.0',
          '@types/react-dom': '^19.0.0',
        },
      },
      null,
      2,
    ),
  );
  execFileSync(
    npm,
    npmArgs(['install', '--ignore-scripts', '--no-audit', '--no-fund']),
    {
      cwd: temp,
      stdio: 'inherit',
    },
  );
  for (const name of ['editor-host', 'editor-consumer']) {
    await copyFile(
      path.join(root, `scripts/packages/fixtures/${name}.ts`),
      path.join(temp, `${name}.ts`),
    );
  }
  const options: ts.CompilerOptions = {
    noEmit: true,
    strict: true,
    skipLibCheck: false,
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    jsx: ts.JsxEmit.ReactJSX,
    lib: ['lib.es2022.d.ts', 'lib.dom.d.ts', 'lib.dom.iterable.d.ts'],
    types: ['react', 'react-dom'],
    typeRoots: [path.join(temp, 'node_modules/@types')],
  };
  const program = ts.createProgram(
    ['editor-host', 'editor-consumer'].map((name) =>
      path.join(temp, `${name}.ts`),
    ),
    options,
  );
  const diagnostics = ts.getPreEmitDiagnostics(program);
  if (diagnostics.length) {
    throw new Error(
      ts.formatDiagnosticsWithColorAndContext(diagnostics, {
        getCanonicalFileName: (name) => name,
        getCurrentDirectory: () => temp,
        getNewLine: () => '\n',
      }),
    );
  }
  const packageCss = await readFile(
    path.join(temp, 'node_modules/@dmnote/editor/dist/editor.css'),
    'utf8',
  );
  if (!packageCss.length) throw new Error('Published editor CSS is empty');
  const scripts: string[] = [];
  // 호스트 스크립트를 먼저 실행해 동적 import 인라인 빌드의 선행 평가와 분리
  for (const name of ['editor-host', 'editor-consumer']) {
    const output = await build({
      configFile: false,
      root: temp,
      logLevel: 'warn',
      define: { 'process.env.NODE_ENV': JSON.stringify('production') },
      build: {
        write: false,
        minify: false,
        lib: {
          entry: path.join(temp, `${name}.ts`),
          formats: ['iife'],
          name: 'DmNotePackageConsumer',
        },
        rollupOptions: { output: { inlineDynamicImports: true } },
      },
    });
    const bundles = Array.isArray(output) ? output : [output];
    const javascript = bundles
      .flatMap((bundle) => ('output' in bundle ? bundle.output : []))
      .filter((chunk) => chunk.type === 'chunk')
      .map((chunk) => chunk.code)
      .join('\n');
    if (!javascript)
      throw new Error('External consumer build produced no JavaScript');
    scripts.push(javascript);
  }
  const errors: unknown[] = [];
  const virtualConsole = new VirtualConsole();
  virtualConsole.on('jsdomError', (error: unknown) => errors.push(error));
  virtualConsole.on('error', (...args: unknown[]) => errors.push(args));
  const dom = new JSDOM(
    '<!doctype html><html><body><div id="root"></div></body></html>',
    {
      url: 'https://external-editor.test/',
      pretendToBeVisual: true,
      runScripts: 'outside-only',
      virtualConsole,
    },
  );
  const channels: MessageChannel[] = [];
  try {
    Object.assign(dom.window, {
      MessageChannel: class extends MessageChannel {
        constructor() {
          super();
          channels.push(this);
        }
      },
      ReadableStream,
      WritableStream,
      TransformStream,
      structuredClone,
      TextEncoder,
      TextDecoder,
      matchMedia: (media: string) => ({
        matches: false,
        media,
        onchange: null,
        addEventListener() {},
        removeEventListener() {},
        addListener() {},
        removeListener() {},
        dispatchEvent: () => true,
      }),
      ResizeObserver: class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    });
    if (!dom.window.CSS)
      Object.assign(dom.window, { CSS: { supports: () => true } });
    for (const script of scripts) dom.window.eval(script);
    const result = await (
      dom.window as unknown as {
        __EDITOR_PACKAGE_CHECK__: Promise<unknown>;
      }
    ).__EDITOR_PACKAGE_CHECK__;
    if (!result) throw new Error('External consumer fixture did not run');
    if (errors.length)
      throw new Error(
        `External consumer runtime errors: ${errors.map(String).join('\n')}`,
      );
    process.stdout.write(
      `External package type/build/runtime check passed: ${JSON.stringify(
        result,
      )}\n`,
    );
  } finally {
    dom.window.close();
    for (const channel of channels) {
      channel.port1.close();
      channel.port2.close();
    }
  }
} finally {
  if (keep) process.stdout.write(`Package consumer retained: ${temp}\n`);
  else await rm(temp, { recursive: true, force: true });
}
