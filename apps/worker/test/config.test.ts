import { describe, it, expect, beforeEach } from 'vitest';
import { env } from 'cloudflare:test';
import { Hono } from 'hono';
import type { Env } from '../src/env';
import config, { _resetConfigCaches } from '../src/routes/config';

/**
 * Tier B — public, unauthenticated reads for `GET /v1/config/vocab` and
 * `GET /v1/config/limits`. Plan #008 W3T12.
 *
 * The admin mutation path is covered separately in `admin.test.ts`; this file
 * focuses on read-path guarantees:
 *   1. Values round-trip from seeded KV,
 *   2. `Cache-Control: public, max-age=60` is set,
 *   3. The public limits view strips compile-time SECURITY invariants
 *      (`MAX_PATH_DEPTH` / `MAX_COMPRESSION_RATIO` / `MAX_PATH_LENGTH`) per
 *      architectural invariant #9b — leaking them would imply they're
 *      runtime-configurable, which they are not.
 */

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

const SEED_VOCAB = { tags: ['dark', 'light', 'minimal'], version: 1 };
const SEED_LIMITS = {
  max_bundle_compressed: 5_242_880,
  max_bundle_uncompressed: 10_485_760,
  max_entries: 32,
  version: 1,
  updated_at: 1_700_000_000,
  // Extra fields to prove the public view strips them — mirror the canonical
  // names of the compile-time security constants in `omni-bundle` so a future
  // accidental exposure would match the explicit "must not be present" asserts
  // below instead of slipping through as an unknown key.
  MAX_PATH_DEPTH: 10,
  MAX_COMPRESSION_RATIO: 100,
  MAX_PATH_LENGTH: 255,
};

function mkApp() {
  const app = new Hono<{ Bindings: Env }>();
  app.route('/v1/config', config);
  return app;
}

function mkGet(path: string): Request {
  return new Request(`https://worker.test${path}`, { method: 'GET' });
}

beforeEach(async () => {
  _resetConfigCaches();
  await env.STATE.put('config:vocab', JSON.stringify(SEED_VOCAB));
  await env.STATE.put('config:limits', JSON.stringify(SEED_LIMITS));
});

describe('GET /v1/config/vocab', () => {
  it('returns seeded KV with Cache-Control: public, max-age=60', async () => {
    const res = await mkApp().fetch(mkGet('/v1/config/vocab'), env);
    expect(res.status).toBe(200);
    expect(res.headers.get('cache-control')).toBe('public, max-age=60');
    const body = (await res.json()) as typeof SEED_VOCAB;
    expect(body.tags).toEqual(SEED_VOCAB.tags);
    expect(body.version).toBe(SEED_VOCAB.version);
  });

  it('500s when KV is unseeded', async () => {
    _resetConfigCaches();
    await env.STATE.delete('config:vocab');
    const res = await mkApp().fetch(mkGet('/v1/config/vocab'), env);
    expect(res.status).toBe(500);
    const json = (await res.json()) as { error: { code: string }; kind: string };
    expect(json.error.code).toBe('SERVER_ERROR');
    expect(json.kind).toBe('Io');
  });
});

describe('GET /v1/config/limits', () => {
  it('returns public view with Cache-Control and without SECURITY invariants', async () => {
    const res = await mkApp().fetch(mkGet('/v1/config/limits'), env);
    expect(res.status).toBe(200);
    expect(res.headers.get('cache-control')).toBe('public, max-age=60');
    const body = (await res.json()) as Record<string, unknown>;
    expect(body.max_bundle_compressed).toBe(SEED_LIMITS.max_bundle_compressed);
    expect(body.max_bundle_uncompressed).toBe(SEED_LIMITS.max_bundle_uncompressed);
    expect(body.max_entries).toBe(SEED_LIMITS.max_entries);
    expect(body.version).toBe(SEED_LIMITS.version);
    expect(body.updated_at).toBe(SEED_LIMITS.updated_at);
    // Architectural invariant #9b: these are compile-time constants in
    // `omni-bundle` and MUST NOT surface on the wire as if runtime-tunable.
    expect('MAX_PATH_DEPTH' in body).toBe(false);
    expect('MAX_COMPRESSION_RATIO' in body).toBe(false);
    expect('MAX_PATH_LENGTH' in body).toBe(false);
  });

  it('500s when KV is unseeded', async () => {
    _resetConfigCaches();
    await env.STATE.delete('config:limits');
    const res = await mkApp().fetch(mkGet('/v1/config/limits'), env);
    expect(res.status).toBe(500);
    const json = (await res.json()) as { error: { code: string }; kind: string };
    expect(json.error.code).toBe('SERVER_ERROR');
    expect(json.kind).toBe('Io');
  });
});
