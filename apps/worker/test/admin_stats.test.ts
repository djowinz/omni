import { describe, it, expect, beforeEach, beforeAll, afterAll } from 'vitest';
import { env, applyD1Migrations } from 'cloudflare:test';
import { Hono } from 'hono';
import * as ed from '@noble/ed25519';
import type { Env } from '../src/env';
import { signJws as signJwsShared } from './helpers/signer';
import admin from '../src/routes/admin';

/**
 * Tier B — Miniflare-backed tests for T9 (#012): GET /v1/admin/stats.
 *
 * Mirrors admin_reports.test.ts / admin_denylist.test.ts fixture pattern.
 * Covers:
 *   - Seeded mixed-state asserts the exact stats shape from sub-spec §3.
 *   - Empty state → all zeros (vocab/limits versions default to 0 when KV
 *     keys absent).
 *   - Non-moderator JWS → Admin.NotModerator 403.
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
  }
});

async function clearKvPrefix(prefix: string): Promise<void> {
  let cursor: string | undefined;
  do {
    const list = await env.STATE.list({ prefix, cursor });
    for (const k of list.keys) await env.STATE.delete(k.name);
    cursor = list.list_complete ? undefined : list.cursor;
  } while (cursor);
}

beforeEach(async () => {
  await env.META.exec('DELETE FROM artifacts');
  await env.META.exec('DELETE FROM authors');
  await clearKvPrefix('reports:');
  await clearKvPrefix('reports-by-status:');
  await clearKvPrefix('denylist:device:');
  await clearKvPrefix('denylist:pubkey:');
  await env.STATE.delete('config:vocab');
  await env.STATE.delete('config:limits');
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = PUBKEY_HEX;
});

afterAll(() => {
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = originalAdminPubkeys;
});

// --- seeders -----------------------------------------------------------

async function seedReport(
  id: string,
  status: 'pending' | 'actioned',
  receivedAt: number,
): Promise<void> {
  const rec = {
    id,
    received_at: receivedAt,
    reporter_pubkey: PUBKEY_HEX,
    reporter_df: DF_HEX,
    artifact_id: `art_${id}`,
    category: 'malware',
    note: null,
    status,
    actioned_by: status === 'actioned' ? PUBKEY_HEX : null,
    action: status === 'actioned' ? 'removed' : null,
  };
  await env.STATE.put(`reports:${id}`, JSON.stringify(rec));
  await env.STATE.put(`reports-by-status:${status}:${receivedAt}:${id}`, id);
}

async function seedAuthor(byteFill: number, isDenied: 0 | 1): Promise<void> {
  const pubkey = new Uint8Array(32).fill(byteFill);
  await env.META.prepare(
    `INSERT OR IGNORE INTO authors (pubkey, display_name, created_at, total_uploads, is_new_creator, is_denied)
     VALUES (?, ?, ?, 0, 0, ?)`,
  )
    .bind(pubkey, `author_${byteFill}`, 1_700_000_000, isDenied)
    .run();
}

async function seedArtifact(
  id: string,
  authorByte: number,
  installCount: number,
  isRemoved: 0 | 1,
): Promise<void> {
  const pubkey = new Uint8Array(32).fill(authorByte);
  await env.META.prepare(
    `INSERT INTO artifacts (
       id, author_pubkey, name, kind, content_hash, thumbnail_hash,
       version, omni_min_version, signature, created_at, updated_at,
       install_count, is_removed
     ) VALUES (?, ?, ?, 'theme', ?, ?, '1.0.0', '0.1.0', x'00', ?, ?, ?, ?)`,
  )
    .bind(
      id,
      pubkey,
      `name_${id}`,
      `hash_${id}`,
      `thumb_${id}`,
      1_700_000_000,
      1_700_000_000,
      installCount,
      isRemoved,
    )
    .run();
}

// --- tests -------------------------------------------------------------

describe('GET /v1/admin/stats', () => {
  it('non-moderator → Admin.NotModerator 403', async () => {
    const app = mkApp();
    const { jws } = await signJws({
      method: 'GET',
      path: '/v1/admin/stats',
      body: new Uint8Array(),
      seedHex: OUTSIDER_SEED_HEX,
    });
    const res = await app.fetch(mkReq('GET', '/v1/admin/stats', new Uint8Array(), jws), env);
    expect(res.status).toBe(403);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Admin');
    expect(j.detail).toBe('NotModerator');
  });

  it('empty state → all zeros, vocab/limits versions default to 0', async () => {
    const app = mkApp();
    const { jws } = await signJws({
      method: 'GET',
      path: '/v1/admin/stats',
      body: new Uint8Array(),
    });
    const res = await app.fetch(mkReq('GET', '/v1/admin/stats', new Uint8Array(), jws), env);
    expect(res.status, await res.clone().text()).toBe(200);
    const body = await res.json();
    expect(body).toEqual({
      pending_reports: 0,
      reviewed_reports: 0,
      actioned_reports: 0,
      banned_pubkeys: 0,
      banned_devices: 0,
      total_artifacts: 0,
      tombstoned_artifacts: 0,
      total_installs: 0,
      vocab_version: 0,
      limits_version: 0,
    });
  });

  it("aggregates seeded mixed state into the spec'd shape", async () => {
    // Reports: 3 pending, 1 actioned.
    await seedReport('r1', 'pending', 1_700_000_001);
    await seedReport('r2', 'pending', 1_700_000_002);
    await seedReport('r3', 'pending', 1_700_000_003);
    await seedReport('r4', 'actioned', 1_700_000_004);

    // Authors: 2 denied, 3 not denied.
    await seedAuthor(0x10, 1);
    await seedAuthor(0x11, 1);
    await seedAuthor(0x20, 0);
    await seedAuthor(0x21, 0);
    await seedAuthor(0x22, 0);

    // Devices: 2 banned.
    await env.STATE.put(
      `denylist:device:${'aa'.repeat(32)}`,
      JSON.stringify({ reason: 'x', at: 1_700_000_000 }),
    );
    await env.STATE.put(
      `denylist:device:${'bb'.repeat(32)}`,
      JSON.stringify({ reason: 'y', at: 1_700_000_000 }),
    );

    // Artifacts: 5 total, 1 tombstoned. Install counts: 10/20/30/0/0 → 60.
    await seedArtifact('a1', 0x20, 10, 0);
    await seedArtifact('a2', 0x20, 20, 0);
    await seedArtifact('a3', 0x21, 30, 0);
    await seedArtifact('a4', 0x21, 0, 0);
    await seedArtifact('a5', 0x22, 0, 1); // tombstoned

    // Vocab/limits config blobs.
    await env.STATE.put('config:vocab', JSON.stringify({ tags: ['minimal', 'neon'], version: 3 }));
    await env.STATE.put(
      'config:limits',
      JSON.stringify({
        max_bundle_compressed: 1024,
        max_bundle_uncompressed: 4096,
        max_entries: 32,
        version: 2,
        updated_at: 1_700_000_000,
      }),
    );

    const app = mkApp();
    const { jws } = await signJws({
      method: 'GET',
      path: '/v1/admin/stats',
      body: new Uint8Array(),
    });
    const res = await app.fetch(mkReq('GET', '/v1/admin/stats', new Uint8Array(), jws), env);
    expect(res.status, await res.clone().text()).toBe(200);
    const body = await res.json();
    expect(body).toEqual({
      pending_reports: 3,
      reviewed_reports: 0,
      actioned_reports: 1,
      banned_pubkeys: 2,
      banned_devices: 2,
      total_artifacts: 4,
      tombstoned_artifacts: 1,
      total_installs: 60,
      vocab_version: 3,
      limits_version: 2,
    });
  });
});
