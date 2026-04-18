import { describe, it, expect, beforeEach, beforeAll, afterAll } from 'vitest';
import { env, applyD1Migrations } from 'cloudflare:test';
import { Hono } from 'hono';
import * as ed from '@noble/ed25519';
import type { Env } from '../src/env';
import { signJws as signJwsShared } from './helpers/signer';
import admin from '../src/routes/admin';
import config, { _resetConfigCaches } from '../src/routes/config';

/**
 * Tier B — Miniflare-backed tests for admin vocab + limits mutations and the
 * public config reads. Plan #008 W3T12.
 *
 * Admin routes are not mounted on the main app until W4T14, so the test mounts
 * them on a local Hono instance. The moderator allowlist (env var
 * `OMNI_ADMIN_PUBKEYS`) is injected here using the same fixture pubkey W1T3
 * + auth.test.ts pins, so any future test re-using the fixture keypair can
 * flip to moderator simply by patching the binding.
 */

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

// Fixture keypair from W1T3 (matches auth.test.ts; same seed → same pubkey).
const SEED_HEX = '0707070707070707070707070707070707070707070707070707070707070707';
const PUBKEY_HEX = 'ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c';
const DF_HEX = 'dc9773ca5d79ecfdedf0c8cca1cfecac9bc39c09550aec75a8cbe8b2a13b67a1';

// A second keypair (seed 0x08 repeated) for the non-moderator case.
const OUTSIDER_SEED_HEX = '0808080808080808080808080808080808080808080808080808080808080808';

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
}
function bytesToHex(b: Uint8Array): string {
  let s = '';
  for (let i = 0; i < b.length; i++) s += b[i]!.toString(16).padStart(2, '0');
  return s;
}

interface SignOpts {
  method: string;
  path: string;
  body: Uint8Array;
  query?: string;
  sanitizeVersion?: number;
  seedHex?: string;
}

async function signJws(o: SignOpts): Promise<{ jws: string; pubkeyHex: string; dfHex: string }> {
  const seedHex = o.seedHex ?? SEED_HEX;
  const seed = hexToBytes(seedHex);
  const pubBytes = await ed.getPublicKeyAsync(seed);
  const pubkeyHex = bytesToHex(pubBytes);
  const dfHex = DF_HEX;
  const jws = await signJwsShared({
    method: o.method,
    path: o.path,
    body: o.body,
    query: o.query,
    seed,
    pubkey: pubBytes,
    df: hexToBytes(dfHex),
    sanitizeVersion: o.sanitizeVersion,
  });
  return { jws, pubkeyHex, dfHex };
}

function mkReq(
  method: string,
  path: string,
  body: Uint8Array,
  jws: string | null,
  extraHeaders: Record<string, string> = {},
): Request {
  const headers = new Headers({ ...extraHeaders });
  if (jws !== null) headers.set('Authorization', `Omni-JWS ${jws}`);
  const init: RequestInit = { method, headers };
  if (method !== 'GET' && method !== 'HEAD') init.body = body;
  return new Request(`https://worker.test${path}`, init);
}

// Build a local Hono app that mounts both routers, matching what W4T14 will
// wire in index.ts.
function mkApp() {
  const app = new Hono<{ Bindings: Env }>();
  app.route('/v1/admin', admin);
  app.route('/v1/config', config);
  return app;
}

// Seed helpers.
const SEED_VOCAB = { tags: ['dark', 'light', 'minimal'], version: 1 };
const SEED_LIMITS = {
  max_bundle_compressed: 5_242_880,
  max_bundle_uncompressed: 10_485_760,
  max_entries: 32,
  version: 1,
  updated_at: 1_700_000_000,
  // Extra field to prove public view strips it.
  MAX_PATH_DEPTH: 10,
  MAX_COMPRESSION_RATIO: 100,
  MAX_PATH_LENGTH: 255,
};

const originalAdminPubkeys = env.OMNI_ADMIN_PUBKEYS;

beforeAll(async () => {
  const migrations = await import('cloudflare:test').then((m) =>
    (m as unknown as { listMigrations?: () => Promise<unknown> }).listMigrations?.(),
  );
  if (migrations) {
    await applyD1Migrations(
      env.META,
      migrations as unknown as Parameters<typeof applyD1Migrations>[1],
    );
  } else {
    // Fallback: inline the bits of 0001_initial_schema.sql the admin tests need.
    // Mirrors report.test.ts's fallback path.
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS authors (pubkey BLOB PRIMARY KEY, display_name TEXT UNIQUE, created_at INTEGER NOT NULL, total_uploads INTEGER NOT NULL DEFAULT 0, is_new_creator INTEGER NOT NULL DEFAULT 1, is_denied INTEGER NOT NULL DEFAULT 0)`,
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS artifacts (id TEXT PRIMARY KEY, author_pubkey BLOB NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, content_hash TEXT NOT NULL, thumbnail_hash TEXT NOT NULL, description TEXT, tags TEXT, license TEXT, version TEXT NOT NULL, omni_min_version TEXT NOT NULL, signature BLOB NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, install_count INTEGER NOT NULL DEFAULT 0, report_count INTEGER NOT NULL DEFAULT 0, is_removed INTEGER NOT NULL DEFAULT 0, is_featured INTEGER NOT NULL DEFAULT 0, UNIQUE (author_pubkey, name))`,
    );
  }
});

beforeEach(async () => {
  // Clean rows from any prior test that seeded artifacts.
  await env.META.exec('DELETE FROM artifacts');
  await env.META.exec('DELETE FROM authors');
  _resetConfigCaches();
  // Reset KV state.
  await env.STATE.put('config:vocab', JSON.stringify(SEED_VOCAB));
  await env.STATE.put('config:limits', JSON.stringify(SEED_LIMITS));
  // Ensure fixture pubkey is denylist-clear so verifyJws passes.
  await env.STATE.delete(`denylist:pubkey:${PUBKEY_HEX}`);
  // Install the fixture as the sole moderator.
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = PUBKEY_HEX;
});

afterAll(() => {
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = originalAdminPubkeys;
});

// ---------------------------------------------------------------------------
// GET /v1/config/vocab + /v1/config/limits
// ---------------------------------------------------------------------------
describe('GET /v1/config/vocab', () => {
  it('returns seeded KV with Cache-Control', async () => {
    const app = mkApp();
    const res = await app.fetch(mkReq('GET', '/v1/config/vocab', new Uint8Array(), null), env);
    expect(res.status).toBe(200);
    expect(res.headers.get('cache-control')).toBe('public, max-age=60');
    const body = (await res.json()) as typeof SEED_VOCAB;
    expect(body.tags).toEqual(SEED_VOCAB.tags);
    expect(body.version).toBe(SEED_VOCAB.version);
  });
});

describe('GET /v1/config/limits', () => {
  it('returns public view without compile-time security constants', async () => {
    const app = mkApp();
    const res = await app.fetch(mkReq('GET', '/v1/config/limits', new Uint8Array(), null), env);
    expect(res.status).toBe(200);
    expect(res.headers.get('cache-control')).toBe('public, max-age=60');
    const body = (await res.json()) as Record<string, unknown>;
    expect(body.max_bundle_compressed).toBe(SEED_LIMITS.max_bundle_compressed);
    expect(body.max_bundle_uncompressed).toBe(SEED_LIMITS.max_bundle_uncompressed);
    expect(body.max_entries).toBe(SEED_LIMITS.max_entries);
    expect(body.version).toBe(SEED_LIMITS.version);
    expect(body.updated_at).toBe(SEED_LIMITS.updated_at);
    // Invariant #9b: compile-time constants MUST NOT leak into the wire body.
    expect('MAX_PATH_DEPTH' in body).toBe(false);
    expect('MAX_COMPRESSION_RATIO' in body).toBe(false);
    expect('MAX_PATH_LENGTH' in body).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// PATCH /v1/admin/vocab
// ---------------------------------------------------------------------------
describe('PATCH /v1/admin/vocab', () => {
  it('non-moderator pubkey → Admin.NotModerator 403', async () => {
    const app = mkApp();
    const body = new TextEncoder().encode(JSON.stringify({ add: ['newtag'] }));
    const { jws } = await signJws({
      method: 'PATCH',
      path: '/v1/admin/vocab',
      body,
      seedHex: OUTSIDER_SEED_HEX,
    });
    const res = await app.fetch(mkReq('PATCH', '/v1/admin/vocab', body, jws), env);
    expect(res.status).toBe(403);
    const json = (await res.json()) as {
      error: { code: string };
      kind: string;
      detail: string;
    };
    expect(json.error.code).toBe('ADMIN_NOT_MODERATOR');
    expect(json.kind).toBe('Admin');
    expect(json.detail).toBe('NotModerator');
  });

  it('moderator add: bumps version + round-trip GET shows the new tag', async () => {
    const app = mkApp();
    const body = new TextEncoder().encode(JSON.stringify({ add: ['newtag'] }));
    const { jws } = await signJws({
      method: 'PATCH',
      path: '/v1/admin/vocab',
      body,
    });
    const res = await app.fetch(mkReq('PATCH', '/v1/admin/vocab', body, jws), env);
    expect(res.status).toBe(200);
    const next = (await res.json()) as typeof SEED_VOCAB;
    expect(next.version).toBe(SEED_VOCAB.version + 1);
    expect(next.tags).toContain('newtag');

    _resetConfigCaches();
    const read = await app.fetch(mkReq('GET', '/v1/config/vocab', new Uint8Array(), null), env);
    const readBody = (await read.json()) as typeof SEED_VOCAB;
    expect(readBody.tags).toContain('newtag');
    expect(readBody.version).toBe(SEED_VOCAB.version + 1);
  });

  it('bad tag (uppercase + space) → Admin.BadTag 400', async () => {
    const app = mkApp();
    const body = new TextEncoder().encode(JSON.stringify({ add: ['Bad Tag'] }));
    const { jws } = await signJws({
      method: 'PATCH',
      path: '/v1/admin/vocab',
      body,
    });
    const res = await app.fetch(mkReq('PATCH', '/v1/admin/vocab', body, jws), env);
    expect(res.status).toBe(400);
    const json = (await res.json()) as { error: { code: string }; detail: string };
    expect(json.error.code).toBe('ADMIN_BAD_TAG');
    expect(json.detail).toBe('BadTag');
  });

  it('empty body {} → Admin.NoOp 400', async () => {
    const app = mkApp();
    const body = new TextEncoder().encode(JSON.stringify({}));
    const { jws } = await signJws({
      method: 'PATCH',
      path: '/v1/admin/vocab',
      body,
    });
    const res = await app.fetch(mkReq('PATCH', '/v1/admin/vocab', body, jws), env);
    expect(res.status).toBe(400);
    const json = (await res.json()) as { error: { code: string }; detail: string };
    expect(json.error.code).toBe('ADMIN_NO_OP');
    expect(json.detail).toBe('NoOp');
  });
});

// ---------------------------------------------------------------------------
// PATCH /v1/admin/limits
// ---------------------------------------------------------------------------
describe('PATCH /v1/admin/limits', () => {
  async function seedArtifact(sizeBytes: number, contentHash: string): Promise<void> {
    // Insert author + artifact row (foreign-key on pubkey).
    const pubBlob = hexToBytes(PUBKEY_HEX);
    await env.META.prepare('INSERT OR IGNORE INTO authors (pubkey, created_at) VALUES (?, ?)')
      .bind(pubBlob, 1_700_000_000)
      .run();
    await env.META.prepare(
      `INSERT INTO artifacts (id, author_pubkey, name, kind, content_hash, thumbnail_hash,
         version, omni_min_version, signature, created_at, updated_at)
       VALUES (?, ?, ?, 'theme', ?, 'thumb-hash', '1.0.0', '0.1.0', x'00', ?, ?)`,
    )
      .bind(
        `artifact-${contentHash.slice(0, 8)}`,
        pubBlob,
        `artifact-${contentHash.slice(0, 8)}`,
        contentHash,
        1_700_000_000,
        1_700_000_000,
      )
      .run();
    // Put a blob of the given size into R2.
    await env.BLOBS.put(contentHash, new Uint8Array(sizeBytes));
  }

  it('lowering below largest artifact without force → WouldOrphanArtifacts 409', async () => {
    await seedArtifact(2000, 'hash-2000-a'.padEnd(64, '0'));
    const app = mkApp();
    const body = new TextEncoder().encode(JSON.stringify({ max_bundle_compressed: 1000 }));
    const { jws } = await signJws({
      method: 'PATCH',
      path: '/v1/admin/limits',
      body,
    });
    const res = await app.fetch(mkReq('PATCH', '/v1/admin/limits', body, jws), env);
    expect(res.status).toBe(409);
    const json = (await res.json()) as { detail: string; error: { code: string } };
    expect(json.detail).toBe('WouldOrphanArtifacts');
    expect(json.error.code).toBe('ADMIN_WOULD_ORPHAN_ARTIFACTS');
  });

  it('lowering below largest artifact WITH X-Omni-Admin-Force → succeeds + version bumps', async () => {
    await seedArtifact(2000, 'hash-2000-b'.padEnd(64, '0'));
    const app = mkApp();
    const body = new TextEncoder().encode(JSON.stringify({ max_bundle_compressed: 1000 }));
    const { jws } = await signJws({
      method: 'PATCH',
      path: '/v1/admin/limits',
      body,
    });
    const res = await app.fetch(
      mkReq('PATCH', '/v1/admin/limits', body, jws, {
        'X-Omni-Admin-Force': 'true',
      }),
      env,
    );
    expect(res.status).toBe(200);
    const next = (await res.json()) as typeof SEED_LIMITS;
    expect(next.max_bundle_compressed).toBe(1000);
    expect(next.version).toBe(SEED_LIMITS.version + 1);
    expect(next.updated_at).toBeGreaterThanOrEqual(SEED_LIMITS.updated_at);
  });

  it('max_bundle_compressed > max_bundle_uncompressed → Admin.BadValue 400', async () => {
    const app = mkApp();
    const body = new TextEncoder().encode(
      JSON.stringify({
        max_bundle_compressed: 999_999_999,
        max_bundle_uncompressed: 1000,
      }),
    );
    const { jws } = await signJws({
      method: 'PATCH',
      path: '/v1/admin/limits',
      body,
    });
    const res = await app.fetch(mkReq('PATCH', '/v1/admin/limits', body, jws), env);
    expect(res.status).toBe(400);
    const json = (await res.json()) as { detail: string; error: { code: string } };
    expect(json.detail).toBe('BadValue');
    expect(json.error.code).toBe('ADMIN_BAD_VALUE');
  });
});
