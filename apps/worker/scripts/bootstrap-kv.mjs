// Bootstrap KV state: writes config:vocab and config:limits to the STATE
// namespace. Run once per deploy environment. Idempotent — re-running
// overwrites with the seed values.
//
// Usage:
//   node scripts/bootstrap-kv.mjs [--remote|--local] [--env <dev|prod|staging>]
//
// Defaults: --remote if no flag, env defaults to prod.

import { execSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const ALLOWED_ENVS = new Set(['prod', 'dev', 'staging']);

const args = process.argv.slice(2);
const localFlag = args.includes('--local') ? '--local' : '--remote';
const envIdx = args.indexOf('--env');
const envName = envIdx >= 0 ? args[envIdx + 1] : 'prod';

if (!ALLOWED_ENVS.has(envName)) {
  console.error(
    `Unknown --env value: ${JSON.stringify(envName)}. Allowed: ${[...ALLOWED_ENVS].join(', ')}`,
  );
  process.exit(1);
}

const envFlag = envName === 'prod' ? '' : `--env ${envName}`;

function run(cmd, label) {
  console.log(`> ${cmd}`);
  try {
    execSync(cmd, { stdio: 'inherit' });
  } catch {
    console.error(`\nFailed at: ${label}`);
    process.exit(1);
  }
}

function seedVocab() {
  const tmpPath = join(tmpdir(), 'omni-vocab-seed.json');
  const body = readFileSync('seed/vocab.json', 'utf8');
  writeFileSync(tmpPath, body);
  run(
    `npx wrangler kv key put --binding=STATE ${localFlag} ${envFlag} config:vocab --path="${tmpPath}"`.trim(),
    'seedVocab',
  );
}

function seedLimits() {
  const tmpPath = join(tmpdir(), 'omni-limits-seed.json');
  const seed = JSON.parse(readFileSync('seed/limits.json', 'utf8'));
  seed.updated_at = Math.floor(Date.now() / 1000);
  writeFileSync(tmpPath, JSON.stringify(seed));
  run(
    `npx wrangler kv key put --binding=STATE ${localFlag} ${envFlag} config:limits --path="${tmpPath}"`.trim(),
    'seedLimits',
  );
}

console.log(`Seeding KV (${localFlag}, env=${envName})...`);
seedVocab();
seedLimits();
console.log('Done.');
