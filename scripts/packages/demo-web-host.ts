import {
  cp,
  mkdtemp,
  realpath,
  rm,
  symlink,
  writeFile,
} from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import os from 'node:os';
import { build, preview } from 'vite';

const root = fileURLToPath(new URL('../../', import.meta.url));
const temp = await realpath(
  await mkdtemp(path.join(os.tmpdir(), 'dmnote-web-demo-')),
);
try {
  await symlink(
    path.join(root, 'node_modules'),
    path.join(temp, 'node_modules'),
  );
  for (const file of [
    'web-demo.tsx',
    'web-demo-overlay.tsx',
    'web-demo-preview.tsx',
  ]) {
    await cp(
      path.join(root, 'scripts/packages/fixtures', file),
      path.join(temp, file),
    );
  }
  await cp(
    path.join(root, 'packages/web-host/dist'),
    path.join(temp, 'public/engine'),
    { recursive: true },
  );
  await writeFile(
    path.join(temp, 'index.html'),
    `<!doctype html><html lang="ko"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>DM Note · 웹 편집 데모</title></head><body><div id="root"></div><script type="module" src="/web-demo.tsx"></script></body></html>`,
  );
  await build({
    configFile: false,
    root: temp,
    logLevel: 'warn',
    build: { target: 'es2022' },
    esbuild: { jsx: 'automatic' },
  });
  const server = await preview({
    configFile: false,
    root: temp,
    preview: { host: '127.0.0.1', port: 4178, strictPort: true },
  });
  server.printUrls();
  process.stdout.write('브라우저 로컬 저장 데모. 종료: Ctrl+C\n');
  await new Promise<void>((resolve) => {
    const close = () => server.httpServer.close(() => resolve());
    process.once('SIGINT', close);
    process.once('SIGTERM', close);
  });
} finally {
  await rm(temp, { recursive: true, force: true });
}
