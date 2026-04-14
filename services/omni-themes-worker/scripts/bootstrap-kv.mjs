// Bootstrap KV state: writes config:vocab and config:limits to the STATE
// namespace. Run once per deploy environment. Idempotent — re-running
// overwrites with the seed values.
//
// Usage:
//   node scripts/bootstrap-kv.mjs [--remote|--local] [--env <dev|prod>]
//
// Defaults: --remote if no flag, env defaults to prod.

import { execSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const args = process.argv.slice(2);
const localFlag = args.includes("--local") ? "--local" : "--remote";
const envIdx = args.indexOf("--env");
const envName = envIdx >= 0 ? args[envIdx + 1] : "prod";
const envFlag = envName === "prod" ? "" : `--env ${envName}`;

function run(cmd) {
  console.log(`> ${cmd}`);
  execSync(cmd, { stdio: "inherit" });
}

function seedVocab() {
  const tmpPath = join(tmpdir(), "omni-vocab-seed.json");
  const body = readFileSync("seed/vocab.json", "utf8");
  writeFileSync(tmpPath, body);
  run(
    `npx wrangler kv:key put --binding=STATE ${localFlag} ${envFlag} config:vocab --path=${tmpPath}`.trim(),
  );
}

function seedLimits() {
  const tmpPath = join(tmpdir(), "omni-limits-seed.json");
  const seed = JSON.parse(readFileSync("seed/limits.json", "utf8"));
  seed.updated_at = Math.floor(Date.now() / 1000);
  writeFileSync(tmpPath, JSON.stringify(seed));
  run(
    `npx wrangler kv:key put --binding=STATE ${localFlag} ${envFlag} config:limits --path=${tmpPath}`.trim(),
  );
}

console.log(`Seeding KV (${localFlag}, env=${envName})...`);
seedVocab();
seedLimits();
console.log("Done.");
