import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const lock = readFileSync(
  new URL('../../src-tauri/Cargo.lock', import.meta.url),
  'utf8',
);
const version = /^name = "wasm-bindgen"\r?\nversion = "([^"]+)"/m.exec(
  lock,
)?.[1];
if (!version)
  throw new Error('Cargo.lock의 wasm-bindgen 버전을 찾을 수 없습니다');
execFileSync('rustup', ['target', 'add', 'wasm32-unknown-unknown'], {
  cwd: root,
  stdio: 'inherit',
});
let installed = '';
try {
  installed = execFileSync('wasm-bindgen', ['--version'], {
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  }).trim();
} catch {
  // 설치되지 않은 CLI는 Cargo.lock과 같은 버전으로 준비
}
if (installed !== `wasm-bindgen ${version}`) {
  execFileSync(
    'cargo',
    ['install', 'wasm-bindgen-cli', '--version', version, '--locked'],
    { cwd: root, stdio: 'inherit' },
  );
}
