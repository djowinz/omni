import { describe, it, expect, beforeEach, beforeAll, afterAll } from 'vitest';
import { env, applyD1Migrations } from 'cloudflare:test';
import { Hono } from 'hono';
import * as ed from '@noble/ed25519';
import type { Env } from '../src/env';
import { signJws as signJwsShared } from './helpers/signer';
import admin from '../src/routes/admin';

/**
 * Tier B — Miniflare-backed tests for T8 (#012): denylist admin endpoints
 *   POST /v1/admin/pubkey/{ban,unban}
 *   POST /v1/admin/device/{ban,unban}
 *
 * Mirrors the admin_artifact.test.ts fixture pattern. Covers:
 *   - pubkey ban: D1 is_denied=1, KV denylist mirror, cascade tombstones all
 *     live artifacts, cascade_count + cascade_errors reported.
 *   - pubkey ban idempotence: rerun → {cascade_count: 0, cascade_errors: 0},
 *     no duplicate tombstone rows.
 *   - pubkey unban: D1 is_denied=0, KV mirror cleared, tombstones retained.
 *   - device ban / unban: KV round-trip.
 *   - non-moderator → Admin.NotModerator 403 on all four.
 *   - invalid hex / missing fields → Malformed.BadRequest 400.
 */

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

const SEED_HEX = '0707070707070707070707070707070707070707070707070707070707070707';
const PUBKEY_HEX = 'ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c';
const DF_HEX = 'dc9773ca5d79ecfdedf0c8cca1cfecac9bc39c09550aec75a8cbe8b2a13b67a1';

const OUTSIDER_SEED_HEX = '0808080808080808080808080808080808080808080808080808080808080808';

// Author to be banned — distinct from the moderator pubkey. 32 bytes of 0xAA.
const BANNED_AUTHOR_HEX = 'aa'.repeat(32);
const BANNED_AUTHOR_BYTES = new Uint8Array(32).fill(0xaa);
const TARGET_DEVICE_HEX = 'bb'.repeat(32);

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
  seedHex?: string;
  dfHex?: string;
}

async function signJws(o: SignOpts): Promise<{ jws: string; pubkeyHex: string }> {
  const seedHex = o.seedHex ?? SEED_HEX;
  const seed = hexToBytes(seedHex);
  const pubBytes = await ed.getPublicKeyAsync(seed);
  const pubkeyHex = bytesToHex(pubBytes);
  const jws = await signJwsShared({
    method: o.method,
    path: o.path,
    body: o.body,
    query: o.query,
    seed,
    pubkey: pubBytes,
    df: hexToBytes(o.dfHex ?? DF_HEX),
  });
  return { jws, pubkeyHex };
}

function mkReq(method: string, path: string, body: Uint8Array, jws: string | null): Request {
  const headers = new Headers();
  if (jws !== null) headers.set('Authorization', `Omni-JWS ${jws}`);
  const init: RequestInit = { method, headers };
  if (method !== 'GET' && method !== 'HEAD') init.body = body;
  return new Request(`https://worker.test${path}`, init);
}

function mkApp() {
  const app = new Hono<{ Bindings: Env }>();
  app.route('/v1/admin', admin);
  return app;
}

const originalAdminPubkeys = env.OMNI_ADMIN_PUBKEYS;

const ART_IDS = ['art_tomb_a', 'art_tomb_b', 'art_tomb_c'];
const CONTENT_HASHES = ['c1', 'c2', 'c3'].map((p) => p + '0'.repeat(62));
const THUMB_HASHES = ['d1', 'd2', 'd3'].map((p) => p + '0'.repeat(62));

async function seedAuthorArtifacts(): Promise<void> {
  await env.META.prepare(
    `INSERT OR IGNORE INTO authors (pubkey, display_name, created_at, total_uploads, is_new_creator, is_denied)
     VALUES (?, 'target', ?, 3, 0, 0)`,
  )
    .bind(BANNED_AUTHOR_BYTES, 1_700_000_000)
    .run();
  for (let i = 0; i < ART_IDS.length; i++) {
    await env.META.prepare(
      `INSERT OR IGNORE INTO artifacts (
         id, author_pubkey, name, kind, content_hash, thumbnail_hash,
         version, omni_min_version, signature, created_at, updated_at
       ) VALUES (?, ?, ?, 'theme', ?, ?, '1.0.0', '0.1.0', x'00', ?, ?)`,
    )
      .bind(
        ART_IDS[i],
        BANNED_AUTHOR_BYTES,
        `name_${ART_IDS[i]}`,
        CONTENT_HASHES[i],
        THUMB_HASHES[i],
        1_700_000_000,
        1_700_000_000,
      )
      .run();
  }
}

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
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS authors (pubkey BLOB PRIMARY KEY, display_name TEXT UNIQUE, created_at INTEGER NOT NULL, total_uploads INTEGER NOT NULL DEFAULT 0, is_new_creator INTEGER NOT NULL DEFAULT 1, is_denied INTEGER NOT NULL DEFAULT 0)`,
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS artifacts (id TEXT PRIMARY KEY, author_pubkey BLOB NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, content_hash TEXT NOT NULL, thumbnail_hash TEXT NOT NULL, description TEXT, tags TEXT, license TEXT, version TEXT NOT NULL, omni_min_version TEXT NOT NULL, signature BLOB NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, install_count INTEGER NOT NULL DEFAULT 0, report_count INTEGER NOT NULL DEFAULT 0, is_removed INTEGER NOT NULL DEFAULT 0, is_featured INTEGER NOT NULL DEFAULT 0, UNIQUE (author_pubkey, name))`,
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS tombstones (content_hash TEXT PRIMARY KEY, reason TEXT, removed_at INTEGER NOT NULL)`,
    );
  }
});

beforeEach(async () => {
  await env.META.exec('DELETE FROM artifacts');
  await env.META.exec('DELETE FROM authors');
  await env.META.exec('DELETE FROM tombstones');
  // Clear KV denylist + moderator-key denylist carryover from sibling suites.
  await env.STATE.delete(`denylist:pubkey:${PUBKEY_HEX}`);
  await env.STATE.delete(`denylist:pubkey:${BANNED_AUTHOR_HEX}`);
  await env.STATE.delete(`denylist:device:${TARGET_DEVICE_HEX}`);
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = PUBKEY_HEX;
});

afterAll(() => {
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = originalAdminPubkeys;
});

// ---------------------------------------------------------------------------
describe('POST /v1/admin/pubkey/ban', () => {
  it('flags is_denied, writes KV mirror, cascades tombstones to all live artifacts', async () => {
    await seedAuthorArtifacts();

    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(
      JSON.stringify({ pubkey: BANNED_AUTHOR_HEX, reason: 'spam ring' }),
    );
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/ban',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/pubkey/ban', bodyBytes, jws), env);
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as {
      pubkey: string;
      status: string;
      cascade_count: number;
      cascade_errors: number;
    };
    expect(body.pubkey).toBe(BANNED_AUTHOR_HEX);
    expect(body.status).toBe('banned');
    expect(body.cascade_count).toBe(3);
    expect(body.cascade_errors).toBe(0);

    // D1 denial flag flipped.
    const authorRow = await env.META.prepare('SELECT is_denied FROM authors WHERE pubkey = ?')
      .bind(BANNED_AUTHOR_BYTES)
      .first<{ is_denied: number }>();
    expect(authorRow?.is_denied).toBe(1);

    // KV mirror present with reason + timestamp.
    const kv = await env.STATE.get(`denylist:pubkey:${BANNED_AUTHOR_HEX}`);
    expect(kv).not.toBeNull();
    const parsed = JSON.parse(kv!);
    expect(parsed.reason).toBe('spam ring');
    expect(typeof parsed.at).toBe('number');

    // All 3 artifacts tombstoned.
    const tombCount = await env.META.prepare('SELECT COUNT(*) AS n FROM tombstones').first<{
      n: number;
    }>();
    expect(tombCount?.n).toBe(3);

    const removedCount = await env.META.prepare(
      'SELECT COUNT(*) AS n FROM artifacts WHERE is_removed = 1',
    ).first<{ n: number }>();
    expect(removedCount?.n).toBe(3);
  });

  it('idempotent: rerun returns cascade_count=0, no duplicate tombstone rows', async () => {
    await seedAuthorArtifacts();

    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(
      JSON.stringify({ pubkey: BANNED_AUTHOR_HEX, reason: 'x' }),
    );
    const s1 = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/ban',
      body: bodyBytes,
    });
    const r1 = await app.fetch(mkReq('POST', '/v1/admin/pubkey/ban', bodyBytes, s1.jws), env);
    expect(r1.status).toBe(200);
    const b1 = (await r1.json()) as { cascade_count: number };
    expect(b1.cascade_count).toBe(3);

    // Re-run.
    const s2 = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/ban',
      body: bodyBytes,
    });
    const r2 = await app.fetch(mkReq('POST', '/v1/admin/pubkey/ban', bodyBytes, s2.jws), env);
    expect(r2.status).toBe(200);
    const b2 = (await r2.json()) as {
      cascade_count: number;
      cascade_errors: number;
    };
    expect(b2.cascade_count).toBe(0);
    expect(b2.cascade_errors).toBe(0);

    // Still exactly 3 tombstones (no duplicates).
    const tombCount = await env.META.prepare('SELECT COUNT(*) AS n FROM tombstones').first<{
      n: number;
    }>();
    expect(tombCount?.n).toBe(3);
  });

  it('non-moderator → Admin.NotModerator 403', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(
      JSON.stringify({ pubkey: BANNED_AUTHOR_HEX, reason: 'x' }),
    );
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/ban',
      body: bodyBytes,
      seedHex: OUTSIDER_SEED_HEX,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/pubkey/ban', bodyBytes, jws), env);
    expect(res.status).toBe(403);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Admin');
    expect(j.detail).toBe('NotModerator');
  });

  it('invalid hex → Malformed.BadRequest', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ pubkey: 'not-hex', reason: 'x' }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/ban',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/pubkey/ban', bodyBytes, jws), env);
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('BadRequest');
  });

  it('missing reason → Malformed.BadRequest', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ pubkey: BANNED_AUTHOR_HEX }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/ban',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/pubkey/ban', bodyBytes, jws), env);
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('BadRequest');
  });
});

describe('POST /v1/admin/pubkey/unban', () => {
  it('clears is_denied + KV mirror; tombstones NOT resurrected', async () => {
    await seedAuthorArtifacts();

    const app = mkApp();
    // First ban to establish state.
    const banBody = new TextEncoder().encode(
      JSON.stringify({ pubkey: BANNED_AUTHOR_HEX, reason: 'x' }),
    );
    const banSig = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/ban',
      body: banBody,
    });
    const banRes = await app.fetch(mkReq('POST', '/v1/admin/pubkey/ban', banBody, banSig.jws), env);
    expect(banRes.status).toBe(200);

    // Now unban.
    const unbanBody = new TextEncoder().encode(JSON.stringify({ pubkey: BANNED_AUTHOR_HEX }));
    const unbanSig = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/unban',
      body: unbanBody,
    });
    const unbanRes = await app.fetch(
      mkReq('POST', '/v1/admin/pubkey/unban', unbanBody, unbanSig.jws),
      env,
    );
    expect(unbanRes.status).toBe(200);
    const ub = (await unbanRes.json()) as { pubkey: string; status: string };
    expect(ub.pubkey).toBe(BANNED_AUTHOR_HEX);
    expect(ub.status).toBe('unbanned');

    // D1 flag cleared.
    const authorRow = await env.META.prepare('SELECT is_denied FROM authors WHERE pubkey = ?')
      .bind(BANNED_AUTHOR_BYTES)
      .first<{ is_denied: number }>();
    expect(authorRow?.is_denied).toBe(0);

    // KV entry gone.
    const kv = await env.STATE.get(`denylist:pubkey:${BANNED_AUTHOR_HEX}`);
    expect(kv).toBeNull();

    // Tombstones still present.
    const tombCount = await env.META.prepare('SELECT COUNT(*) AS n FROM tombstones').first<{
      n: number;
    }>();
    expect(tombCount?.n).toBe(3);
    const stillRemoved = await env.META.prepare(
      'SELECT COUNT(*) AS n FROM artifacts WHERE is_removed = 1',
    ).first<{ n: number }>();
    expect(stillRemoved?.n).toBe(3);
  });

  it('non-moderator → Admin.NotModerator 403', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ pubkey: BANNED_AUTHOR_HEX }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/unban',
      body: bodyBytes,
      seedHex: OUTSIDER_SEED_HEX,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/pubkey/unban', bodyBytes, jws), env);
    expect(res.status).toBe(403);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Admin');
    expect(j.detail).toBe('NotModerator');
  });

  it('invalid hex → Malformed.BadRequest', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ pubkey: 'zz' }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/pubkey/unban',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/pubkey/unban', bodyBytes, jws), env);
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('BadRequest');
  });
});

describe('POST /v1/admin/device/ban', () => {
  it('writes KV denylist entry with reason + at', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(
      JSON.stringify({ device_fp: TARGET_DEVICE_HEX, reason: 'botnet' }),
    );
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/device/ban',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/device/ban', bodyBytes, jws), env);
    expect(res.status, await res.clone().text()).toBe(200);
    const b = (await res.json()) as { device_fp: string; status: string };
    expect(b.device_fp).toBe(TARGET_DEVICE_HEX);
    expect(b.status).toBe('banned');

    const kv = await env.STATE.get(`denylist:device:${TARGET_DEVICE_HEX}`);
    expect(kv).not.toBeNull();
    const parsed = JSON.parse(kv!);
    expect(parsed.reason).toBe('botnet');
    expect(typeof parsed.at).toBe('number');
  });

  it('non-moderator → Admin.NotModerator 403', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(
      JSON.stringify({ device_fp: TARGET_DEVICE_HEX, reason: 'x' }),
    );
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/device/ban',
      body: bodyBytes,
      seedHex: OUTSIDER_SEED_HEX,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/device/ban', bodyBytes, jws), env);
    expect(res.status).toBe(403);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Admin');
    expect(j.detail).toBe('NotModerator');
  });

  it('invalid hex → Malformed.BadRequest', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ device_fp: 'short', reason: 'x' }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/device/ban',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/device/ban', bodyBytes, jws), env);
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('BadRequest');
  });
});

describe('POST /v1/admin/device/unban', () => {
  it('deletes the KV denylist entry', async () => {
    // Seed the denylist directly.
    await env.STATE.put(
      `denylist:device:${TARGET_DEVICE_HEX}`,
      JSON.stringify({ reason: 'x', at: 1 }),
    );

    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ device_fp: TARGET_DEVICE_HEX }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/device/unban',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/device/unban', bodyBytes, jws), env);
    expect(res.status).toBe(200);
    const b = (await res.json()) as { device_fp: string; status: string };
    expect(b.device_fp).toBe(TARGET_DEVICE_HEX);
    expect(b.status).toBe('unbanned');

    const kv = await env.STATE.get(`denylist:device:${TARGET_DEVICE_HEX}`);
    expect(kv).toBeNull();
  });

  it('non-moderator → Admin.NotModerator 403', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ device_fp: TARGET_DEVICE_HEX }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/device/unban',
      body: bodyBytes,
      seedHex: OUTSIDER_SEED_HEX,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/device/unban', bodyBytes, jws), env);
    expect(res.status).toBe(403);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Admin');
    expect(j.detail).toBe('NotModerator');
  });

  it('invalid hex → Malformed.BadRequest', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ device_fp: 42 }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/device/unban',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/device/unban', bodyBytes, jws), env);
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('BadRequest');
  });
});
