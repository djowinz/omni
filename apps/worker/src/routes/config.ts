import { Hono } from 'hono';
import type { AppEnv } from '../types';
import { errorResponse } from '../lib/errors';
import { makeDebugLog } from '../lib/debug-log';
import type { KVNamespace } from '@cloudflare/workers-types';

/**
 * Public, unauthenticated reads of the runtime-tunable config. Values are
 * seeded by `scripts/bootstrap-kv.mjs` (#007) and mutated by the admin routes
 * in `src/routes/admin.ts` (#008).
 *
 * Caching posture: edge-cacheable 60s (`Cache-Control: public, max-age=60`)
 * plus an in-memory module cache so each isolate serves the hot path without
 * hitting KV. Spec §4a/4b, contract §4.9/4.10.
 *
 * Security invariants (`MAX_PATH_DEPTH`, `MAX_COMPRESSION_RATIO`,
 * `MAX_PATH_LENGTH`) are deliberately NOT exposed here — they are compile-time
 * `pub const` in `omni-bundle` per architectural invariant #9b. Stripping them
 * from the limits body is load-bearing: tooling must not be able to assume
 * they're runtime-configurable.
 */

const CACHE_TTL_MS = 60_000;

interface Cached<T> {
  value: T;
  expiresAt: number;
}

let vocabCache: Cached<unknown> | null = null;
let limitsCache: Cached<LimitsPublic> | null = null;

export interface LimitsPublic {
  max_bundle_compressed: number;
  max_bundle_uncompressed: number;
  max_entries: number;
  version: number;
  updated_at: number;
}

/** Internal KV shape may carry additional fields; public view strips them. */
interface LimitsStored extends LimitsPublic {
  [k: string]: unknown;
}

function publicLimitsView(stored: LimitsStored): LimitsPublic {
  return {
    max_bundle_compressed: stored.max_bundle_compressed,
    max_bundle_uncompressed: stored.max_bundle_uncompressed,
    max_entries: stored.max_entries,
    version: stored.version,
    updated_at: stored.updated_at,
  };
}

/** Test-only reset hook — keeps per-test isolation when KV is mutated. */
export function _resetConfigCaches(): void {
  vocabCache = null;
  limitsCache = null;
}

async function readVocab(kv: KVNamespace): Promise<unknown | null> {
  const now = Date.now();
  if (vocabCache && vocabCache.expiresAt > now) return vocabCache.value;
  const raw = await kv.get('config:vocab', 'json');
  if (raw === null) return null;
  vocabCache = { value: raw, expiresAt: now + CACHE_TTL_MS };
  return raw;
}

async function readLimits(kv: KVNamespace): Promise<LimitsPublic | null> {
  const now = Date.now();
  if (limitsCache && limitsCache.expiresAt > now) return limitsCache.value;
  const raw = (await kv.get('config:limits', 'json')) as LimitsStored | null;
  if (raw === null) return null;
  const view = publicLimitsView(raw);
  limitsCache = { value: view, expiresAt: now + CACHE_TTL_MS };
  return view;
}

const app = new Hono<AppEnv>();

app.get('/vocab', async (c) => {
  makeDebugLog(c.env)(`[config] GET /v1/config/vocab`);
  const value = await readVocab(c.env.STATE);
  if (value === null) {
    return errorResponse(500, 'SERVER_ERROR', 'config:vocab not seeded', {
      kind: 'Io',
    });
  }
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      'cache-control': 'public, max-age=60',
    },
  });
});

app.get('/limits', async (c) => {
  makeDebugLog(c.env)(`[config] GET /v1/config/limits`);
  const value = await readLimits(c.env.STATE);
  if (value === null) {
    return errorResponse(500, 'SERVER_ERROR', 'config:limits not seeded', {
      kind: 'Io',
    });
  }
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      'cache-control': 'public, max-age=60',
    },
  });
});

export default app;
