#!/usr/bin/env node
import { execSync } from "node:child_process";
import { mkdirSync, copyFileSync } from "node:fs";
import { resolve } from "node:path";

const repo = resolve(process.cwd(), "../..");
const out = resolve(process.cwd(), "src/wasm");
mkdirSync(out, { recursive: true });

for (const crate of ["omni-bundle", "omni-identity", "omni-sanitize"]) {
  console.log(`[wasm] building ${crate}`);
  execSync(
    `wasm-pack build --target web --no-typescript --release --out-dir pkg --features wasm`,
    { cwd: resolve(repo, "crates", crate), stdio: "inherit" },
  );
  const snake = crate.replace(/-/g, "_");
  copyFileSync(resolve(repo, "crates", crate, "pkg", `${snake}_bg.wasm`), resolve(out, `${snake}.wasm`));
  copyFileSync(resolve(repo, "crates", crate, "pkg", `${snake}.js`), resolve(out, `${snake}.js`));
}
console.log("[wasm] done");
