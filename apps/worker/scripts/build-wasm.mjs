#!/usr/bin/env node
import { execSync } from 'node:child_process';
import { mkdirSync, copyFileSync } from 'node:fs';
import { resolve } from 'node:path';

const repo = resolve(process.cwd(), '../..');
const out = resolve(process.cwd(), 'src/wasm');
mkdirSync(out, { recursive: true });

// Crate folder names simplified after the monorepo restructure (no `omni-`
// prefix). Consumer code in apps/worker still imports the WASM shims by
// `omni_*` filenames — preserve those output names for back-compat.
const builds = [
  { dir: 'bundle', outName: 'omni_bundle' },
  { dir: 'identity', outName: 'omni_identity' },
  { dir: 'sanitize', outName: 'omni_sanitize' },
];

for (const { dir, outName } of builds) {
  console.log(`[wasm] building ${dir}`);
  execSync(`wasm-pack build --target web --no-typescript --release --out-dir pkg --features wasm`, {
    cwd: resolve(repo, 'crates', dir),
    stdio: 'inherit',
  });
  const snake = dir.replace(/-/g, '_');
  copyFileSync(
    resolve(repo, 'crates', dir, 'pkg', `${snake}_bg.wasm`),
    resolve(out, `${outName}.wasm`),
  );
  copyFileSync(resolve(repo, 'crates', dir, 'pkg', `${snake}.js`), resolve(out, `${outName}.js`));
}
console.log('[wasm] done');
