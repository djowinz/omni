import { describe, it, expect, beforeEach, beforeAll, afterAll } from 'vitest';
import { env, applyD1Migrations } from 'cloudflare:test';
import { Hono } from 'hono';
import * as ed from '@noble/ed25519';
import type { Env } from '../src/env';
import { signJws as signJwsShared } from './helpers/signer';
import admin from '../src/routes/admin';

/**
 * Tier B — Miniflare-backed tests for T7 (#012): POST /v1/admin/artifact/:id/remove.
 *
 * Mirrors the admin_reports.test.ts fixture pattern. Validates:
 *   - tombstone row written + is_removed flipped + R2 blob deleted
 *   - response `{artifact_id, status: "removed"}`
 *   - idempotence (second call → `already_tombstoned`, no duplicate side effects)
 *   - unknown id → Malformed.NotFound 404
 *   - non-moderator JWS → Admin.NotModerator 403
 *   - missing / non-string / empty `reason` → Malformed.BadRequest
 *   - upload-side tombstone enforcement: after admin-remove, the D1
 *     `tombstones` row is the exact shape `upload.ts` SELECTs on, so
 *     re-upload of identical content_hash is rejected by #008's check.
 */

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

const SEED_HEX = '0707070707070707070707070707070707070707070707070707070707070707';
const PUBKEY_HEX = 'ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c';
const DF_HEX = 'dc9773ca5d79ecfdedf0c8cca1cfecac9bc39c09550aec75a8cbe8b2a13b67a1';

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
const AUTHOR_PUBKEY_BYTES = new Uint8Array(32).fill(0xaa);

const ART_ID = 'art_tomb';
const CONTENT_HASH = 'c'.repeat(64);
const THUMB_HASH = 'd'.repeat(64);
const BUNDLE_KEY = `bundles/${CONTENT_HASH}.omnipkg`;
const THUMB_KEY = `thumbnails/${THUMB_HASH}.png`;

async function seedArtifact(
  opts: {
    id?: string;
    contentHash?: string;
    thumbHash?: string;
  } = {},
): Promise<void> {
  const id = opts.id ?? ART_ID;
  const contentHash = opts.contentHash ?? CONTENT_HASH;
  const thumbHash = opts.thumbHash ?? THUMB_HASH;
  await env.META.prepare(
    `INSERT OR IGNORE INTO authors (pubkey, display_name, created_at) VALUES (?, ?, ?)`,
  )
    .bind(AUTHOR_PUBKEY_BYTES, `author_${id}`, 1_700_000_000)
    .run();
  await env.META.prepare(
    `INSERT OR IGNORE INTO artifacts (
       id, author_pubkey, name, kind, content_hash, thumbnail_hash,
       version, omni_min_version, signature, created_at, updated_at
     ) VALUES (?, ?, ?, 'theme', ?, ?, '1.0.0', '0.1.0', x'00', ?, ?)`,
  )
    .bind(
      id,
      AUTHOR_PUBKEY_BYTES,
      `name_${id}`,
      contentHash,
      thumbHash,
      1_700_000_000,
      1_700_000_000,
    )
    .run();
}

async function putBlob(key: string, body: Uint8Array): Promise<void> {
  await env.BLOBS.put(key, body);
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
      `CREATE TABLE IF NOT EXISTS authors (pubkey BLOB PRIMARY KEY, display_name TEXT, created_at INTEGER NOT NULL, total_uploads INTEGER NOT NULL DEFAULT 0, is_new_creator INTEGER NOT NULL DEFAULT 1, is_denied INTEGER NOT NULL DEFAULT 0)`,
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
  // Clear R2 of the fixture keys (best-effort; these are the only keys the
  // tests touch).
  for (const key of [BUNDLE_KEY, THUMB_KEY]) {
    try {
      await env.BLOBS.delete(key);
    } catch {
      /* ignore */
    }
  }
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = PUBKEY_HEX;
});

afterAll(() => {
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = originalAdminPubkeys;
});

// ---------------------------------------------------------------------------
describe('POST /v1/admin/artifact/:id/remove', () => {
  it('tombstones artifact: is_removed=1, tombstone row written, R2 deleted', async () => {
    await seedArtifact();
    await putBlob(BUNDLE_KEY, new Uint8Array([0x01, 0x02, 0x03]));
    await putBlob(THUMB_KEY, new Uint8Array([0x89, 0x50, 0x4e, 0x47]));

    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ reason: 'copyright violation' }));
    const { jws } = await signJws({
      method: 'POST',
      path: `/v1/admin/artifact/${ART_ID}/remove`,
      body: bodyBytes,
    });
    const res = await app.fetch(
      mkReq('POST', `/v1/admin/artifact/${ART_ID}/remove`, bodyBytes, jws),
      env,
    );
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as {
      artifact_id: string;
      status: string;
      content_hash: string;
    };
    expect(body.artifact_id).toBe(ART_ID);
    expect(body.status).toBe('removed');
    expect(body.content_hash).toBe(CONTENT_HASH);

    const row = await env.META.prepare('SELECT is_removed FROM artifacts WHERE id = ?')
      .bind(ART_ID)
      .first<{ is_removed: number }>();
    expect(row?.is_removed).toBe(1);

    const tomb = await env.META.prepare(
      'SELECT content_hash, reason FROM tombstones WHERE content_hash = ?',
    )
      .bind(CONTENT_HASH)
      .first<{ content_hash: string; reason: string }>();
    expect(tomb?.content_hash).toBe(CONTENT_HASH);
    expect(tomb?.reason).toBe('copyright violation');

    const bundleObj = await env.BLOBS.get(BUNDLE_KEY);
    expect(bundleObj).toBeNull();
    const thumbObj = await env.BLOBS.get(THUMB_KEY);
    expect(thumbObj).toBeNull();
  });

  it('idempotent: re-run returns already_tombstoned, no extra side effects', async () => {
    await seedArtifact();
    await putBlob(BUNDLE_KEY, new Uint8Array([0x01]));

    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ reason: 'x' }));

    const s1 = await signJws({
      method: 'POST',
      path: `/v1/admin/artifact/${ART_ID}/remove`,
      body: bodyBytes,
    });
    const res1 = await app.fetch(
      mkReq('POST', `/v1/admin/artifact/${ART_ID}/remove`, bodyBytes, s1.jws),
      env,
    );
    expect(res1.status).toBe(200);

    // Second call — tombstone row + is_removed already in place.
    const s2 = await signJws({
      method: 'POST',
      path: `/v1/admin/artifact/${ART_ID}/remove`,
      body: bodyBytes,
    });
    const res2 = await app.fetch(
      mkReq('POST', `/v1/admin/artifact/${ART_ID}/remove`, bodyBytes, s2.jws),
      env,
    );
    expect(res2.status).toBe(200);
    const body2 = (await res2.json()) as {
      status: string;
      content_hash: string;
    };
    expect(body2.status).toBe('already_tombstoned');
    expect(body2.content_hash).toBe(CONTENT_HASH);

    // Still exactly one tombstone row.
    const count = await env.META.prepare(
      'SELECT COUNT(*) AS n FROM tombstones WHERE content_hash = ?',
    )
      .bind(CONTENT_HASH)
      .first<{ n: number }>();
    expect(count?.n).toBe(1);
  });

  it('unknown id → Malformed.NotFound 404', async () => {
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ reason: 'x' }));
    const { jws } = await signJws({
      method: 'POST',
      path: `/v1/admin/artifact/nonexistent/remove`,
      body: bodyBytes,
    });
    const res = await app.fetch(
      mkReq('POST', `/v1/admin/artifact/nonexistent/remove`, bodyBytes, jws),
      env,
    );
    expect(res.status).toBe(404);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('NotFound');
  });

  it('non-moderator → Admin.NotModerator 403', async () => {
    await seedArtifact();
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ reason: 'x' }));
    const { jws } = await signJws({
      method: 'POST',
      path: `/v1/admin/artifact/${ART_ID}/remove`,
      body: bodyBytes,
      seedHex: OUTSIDER_SEED_HEX,
    });
    const res = await app.fetch(
      mkReq('POST', `/v1/admin/artifact/${ART_ID}/remove`, bodyBytes, jws),
      env,
    );
    expect(res.status).toBe(403);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Admin');
    expect(j.detail).toBe('NotModerator');
  });

  it('missing reason → Malformed.BadRequest', async () => {
    await seedArtifact();
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({}));
    const { jws } = await signJws({
      method: 'POST',
      path: `/v1/admin/artifact/${ART_ID}/remove`,
      body: bodyBytes,
    });
    const res = await app.fetch(
      mkReq('POST', `/v1/admin/artifact/${ART_ID}/remove`, bodyBytes, jws),
      env,
    );
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('BadRequest');
  });

  it('empty reason → Malformed.BadRequest', async () => {
    await seedArtifact();
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ reason: '' }));
    const { jws } = await signJws({
      method: 'POST',
      path: `/v1/admin/artifact/${ART_ID}/remove`,
      body: bodyBytes,
    });
    const res = await app.fetch(
      mkReq('POST', `/v1/admin/artifact/${ART_ID}/remove`, bodyBytes, jws),
      env,
    );
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('BadRequest');
  });

  it('non-string reason → Malformed.BadRequest', async () => {
    await seedArtifact();
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ reason: 42 }));
    const { jws } = await signJws({
      method: 'POST',
      path: `/v1/admin/artifact/${ART_ID}/remove`,
      body: bodyBytes,
    });
    const res = await app.fetch(
      mkReq('POST', `/v1/admin/artifact/${ART_ID}/remove`, bodyBytes, jws),
      env,
    );
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('BadRequest');
  });

  it('written tombstone row matches upload-side SELECT shape (re-upload rejection wiring)', async () => {
    // This validates the contract with #008's upload.ts tombstone guard:
    //   SELECT content_hash FROM tombstones WHERE content_hash = ?
    // If this row exists with `content_hash` matching the canonical hash of
    // an attempted upload, the upload route returns Integrity.Tombstoned 410.
    // Seeding through the admin handler here proves the wiring end-to-end.
    await seedArtifact();
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ reason: 'malware' }));
    const { jws } = await signJws({
      method: 'POST',
      path: `/v1/admin/artifact/${ART_ID}/remove`,
      body: bodyBytes,
    });
    const res = await app.fetch(
      mkReq('POST', `/v1/admin/artifact/${ART_ID}/remove`, bodyBytes, jws),
      env,
    );
    expect(res.status).toBe(200);

    // Exact shape upload.ts SELECTs on.
    const hit = await env.META.prepare('SELECT content_hash FROM tombstones WHERE content_hash = ?')
      .bind(CONTENT_HASH)
      .first<{ content_hash: string }>();
    expect(hit).not.toBeNull();
    expect(hit?.content_hash).toBe(CONTENT_HASH);
  });
});
