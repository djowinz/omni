import { describe, it, expect, beforeEach, beforeAll, afterAll } from 'vitest';
import { env, applyD1Migrations } from 'cloudflare:test';
import { Hono } from 'hono';
import * as ed from '@noble/ed25519';
import type { Env } from '../src/env';
import { signJws as signJwsShared } from './helpers/signer';
import admin from '../src/routes/admin';
import report from '../src/routes/report';

/**
 * Tier B — Miniflare-backed tests for T6 (#012): admin reports queue endpoints
 * and the evolved `POST /v1/report` record shape + secondary-index write.
 *
 * Mirrors the fixture pattern in admin.test.ts / report.test.ts: seed
 * OMNI_ADMIN_PUBKEYS with the W1T3 fixture pubkey, mount admin + report on a
 * local Hono app, and drive requests via `app.fetch(req, env)`.
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
  app.route('/v1/report', report);
  return app;
}

const originalAdminPubkeys = env.OMNI_ADMIN_PUBKEYS;
const OTHER_PUBKEY_BYTES = new Uint8Array(32).fill(0xaa);

async function seedArtifact(id: string): Promise<void> {
  await env.META.prepare(
    `INSERT OR IGNORE INTO authors (pubkey, display_name, created_at) VALUES (?, ?, ?)`,
  )
    .bind(OTHER_PUBKEY_BYTES, `author_${id}`, 1_700_000_000)
    .run();
  await env.META.prepare(
    `INSERT OR IGNORE INTO artifacts (
       id, author_pubkey, name, kind, content_hash, thumbnail_hash,
       version, omni_min_version, signature, created_at, updated_at
     ) VALUES (?, ?, ?, 'theme', ?, ?, '1.0.0', '0.1.0', x'00', ?, ?)`,
  )
    .bind(
      id,
      OTHER_PUBKEY_BYTES,
      `name_${id}`,
      `hash_${id}`,
      `thumb_${id}`,
      1_700_000_000,
      1_700_000_000,
    )
    .run();
}

/** Directly insert a report record + its secondary-index key. Faster than
 *  routing through POST /v1/report (which takes the rate-limit path). */
async function seedReport(opts: {
  id: string;
  artifactId: string;
  receivedAt: number;
  category?: string;
}): Promise<void> {
  const rec = {
    id: opts.id,
    received_at: opts.receivedAt,
    reporter_pubkey: PUBKEY_HEX,
    reporter_df: DF_HEX,
    artifact_id: opts.artifactId,
    category: opts.category ?? 'malware',
    note: null,
    status: 'pending' as const,
    actioned_by: null,
    action: null,
  };
  await env.STATE.put(`reports:${opts.id}`, JSON.stringify(rec));
  await env.STATE.put(`reports-by-status:pending:${opts.receivedAt}:${opts.id}`, opts.id);
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
  }
});

async function clearReports(): Promise<void> {
  for (const prefix of ['reports:', 'reports-by-status:']) {
    let cursor: string | undefined;
    do {
      const list = await env.STATE.list({ prefix, cursor });
      for (const k of list.keys) await env.STATE.delete(k.name);
      cursor = list.list_complete ? undefined : list.cursor;
    } while (cursor);
  }
}

beforeEach(async () => {
  await env.META.exec('DELETE FROM artifacts');
  await env.META.exec('DELETE FROM authors');
  await clearReports();
  await env.STATE.delete(`denylist:pubkey:${PUBKEY_HEX}`);
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = PUBKEY_HEX;
});

afterAll(() => {
  (env as unknown as { OMNI_ADMIN_PUBKEYS: string }).OMNI_ADMIN_PUBKEYS = originalAdminPubkeys;
});

// ---------------------------------------------------------------------------
// POST /v1/report — secondary index + extended record shape.
// ---------------------------------------------------------------------------
describe('POST /v1/report (T6 record shape)', () => {
  it('writes reports:<id> + reports-by-status:pending:<ts>:<id>', async () => {
    await seedArtifact('art_index_check');
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(
      JSON.stringify({ artifact_id: 'art_index_check', category: 'other' }),
    );
    // Use a fresh DF so no quota bleed.
    const dfHex = '11'.repeat(32);
    await env.STATE.delete(`df_pubkey_velocity:${dfHex}`);
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/report',
      body: bodyBytes,
      dfHex,
    });
    const res = await app.fetch(mkReq('POST', '/v1/report', bodyBytes, jws), env);
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as { report_id: string };

    const rec = JSON.parse((await env.STATE.get(`reports:${body.report_id}`))!);
    expect(rec.id).toBe(body.report_id);
    expect(rec.status).toBe('pending');
    expect(rec.actioned_by).toBeNull();
    expect(rec.action).toBeNull();

    const idx = await env.STATE.list({
      prefix: `reports-by-status:pending:`,
    });
    const match = idx.keys.find((k) => k.name.endsWith(`:${body.report_id}`));
    expect(match).toBeDefined();
  });
});

// ---------------------------------------------------------------------------
// GET /v1/admin/reports
// ---------------------------------------------------------------------------
describe('GET /v1/admin/reports', () => {
  it('non-moderator → Admin.NotModerator 403', async () => {
    const app = mkApp();
    const { jws } = await signJws({
      method: 'GET',
      path: '/v1/admin/reports',
      body: new Uint8Array(),
      query: 'status=pending',
      seedHex: OUTSIDER_SEED_HEX,
    });
    const res = await app.fetch(
      mkReq('GET', '/v1/admin/reports?status=pending', new Uint8Array(), jws),
      env,
    );
    expect(res.status).toBe(403);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Admin');
    expect(j.detail).toBe('NotModerator');
  });

  it('returns paginated items + next_cursor when results exceed limit', async () => {
    await seedArtifact('art_page');
    for (let i = 0; i < 5; i++) {
      await seedReport({
        id: `r${i}`,
        artifactId: 'art_page',
        receivedAt: 1_700_000_000 + i,
      });
    }
    const app = mkApp();
    const { jws } = await signJws({
      method: 'GET',
      path: '/v1/admin/reports',
      body: new Uint8Array(),
      query: 'status=pending&limit=2',
    });
    const res = await app.fetch(
      mkReq('GET', '/v1/admin/reports?status=pending&limit=2', new Uint8Array(), jws),
      env,
    );
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as {
      items: Array<{ id: string; reason: string; notes: string | null }>;
      next_cursor?: string;
    };
    expect(body.items.length).toBe(2);
    expect(body.next_cursor).toBeTruthy();
    // Admin view translates stored category/note → contract reason/notes.
    expect(body.items[0]!.reason).toBe('malware');
    expect(body.items[0]!.notes).toBeNull();
  });

  it('no status filter returns reports across all statuses', async () => {
    await seedArtifact('art_all');
    // Seed one pending, one reviewed, one actioned — directly via KV so we
    // don't depend on the action-POST path.
    const mkRec = (
      id: string,
      receivedAt: number,
      status: 'pending' | 'reviewed' | 'actioned',
    ) => ({
      id,
      received_at: receivedAt,
      reporter_pubkey: PUBKEY_HEX,
      reporter_df: DF_HEX,
      artifact_id: 'art_all',
      category: 'malware',
      note: null,
      status,
      actioned_by: status === 'pending' ? null : PUBKEY_HEX,
      action: status === 'pending' ? null : 'no_action',
    });
    for (const [id, ts, status] of [
      ['r_p', 1_700_000_900, 'pending'],
      ['r_r', 1_700_001_000, 'reviewed'],
      ['r_a', 1_700_001_100, 'actioned'],
    ] as const) {
      await env.STATE.put(`reports:${id}`, JSON.stringify(mkRec(id, ts, status)));
      await env.STATE.put(`reports-by-status:${status}:${ts}:${id}`, id);
    }

    const app = mkApp();
    const { jws } = await signJws({
      method: 'GET',
      path: '/v1/admin/reports',
      body: new Uint8Array(),
    });
    const res = await app.fetch(mkReq('GET', '/v1/admin/reports', new Uint8Array(), jws), env);
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as {
      items: Array<{ id: string; status: string }>;
    };
    const ids = body.items.map((i) => i.id).sort();
    expect(ids).toEqual(['r_a', 'r_p', 'r_r']);
  });
});

// ---------------------------------------------------------------------------
// GET /v1/admin/report/:id
// ---------------------------------------------------------------------------
describe('GET /v1/admin/report/:id', () => {
  it('returns joined {report, linked_artifact}', async () => {
    await seedArtifact('art_show');
    await seedReport({
      id: 'r_show',
      artifactId: 'art_show',
      receivedAt: 1_700_000_100,
    });
    const app = mkApp();
    const { jws } = await signJws({
      method: 'GET',
      path: '/v1/admin/report/r_show',
      body: new Uint8Array(),
    });
    const res = await app.fetch(
      mkReq('GET', '/v1/admin/report/r_show', new Uint8Array(), jws),
      env,
    );
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as {
      report: { id: string; artifact_id: string; reason: string };
      linked_artifact: { id: string } | null;
    };
    expect(body.report.id).toBe('r_show');
    expect(body.report.reason).toBe('malware');
    expect(body.linked_artifact?.id).toBe('art_show');
  });

  it('unknown id → Malformed.NotFound 404', async () => {
    const app = mkApp();
    const { jws } = await signJws({
      method: 'GET',
      path: '/v1/admin/report/nonexistent',
      body: new Uint8Array(),
    });
    const res = await app.fetch(
      mkReq('GET', '/v1/admin/report/nonexistent', new Uint8Array(), jws),
      env,
    );
    expect(res.status).toBe(404);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('NotFound');
  });
});

// ---------------------------------------------------------------------------
// POST /v1/admin/report/:id/action
// ---------------------------------------------------------------------------
describe('POST /v1/admin/report/:id/action', () => {
  it('no_action transitions pending → reviewed + swaps index key', async () => {
    await seedArtifact('art_a');
    await seedReport({
      id: 'r_a',
      artifactId: 'art_a',
      receivedAt: 1_700_000_200,
    });
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(
      JSON.stringify({ action: 'no_action', notes: 'looks fine' }),
    );
    const { jws, pubkeyHex } = await signJws({
      method: 'POST',
      path: '/v1/admin/report/r_a/action',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/report/r_a/action', bodyBytes, jws), env);
    expect(res.status, await res.clone().text()).toBe(200);
    // Contract §4.15: response is the updated report object directly.
    const body = (await res.json()) as {
      status: string;
      actioned_by: string;
      action: string;
      action_notes?: string;
    };
    expect(body.status).toBe('reviewed');
    expect(body.actioned_by).toBe(pubkeyHex);
    expect(body.action).toBe('no_action');
    expect(body.action_notes).toBe('looks fine');

    // Secondary index: old pending key deleted, new reviewed key present.
    const oldKey = `reports-by-status:pending:1700000200:r_a`;
    const newKey = `reports-by-status:reviewed:1700000200:r_a`;
    expect(await env.STATE.get(oldKey)).toBeNull();
    expect(await env.STATE.get(newKey)).toBe('r_a');
  });

  it('removed action transitions to actioned (not reviewed)', async () => {
    await seedArtifact('art_b');
    await seedReport({
      id: 'r_b',
      artifactId: 'art_b',
      receivedAt: 1_700_000_300,
    });
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ action: 'removed' }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/report/r_b/action',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/report/r_b/action', bodyBytes, jws), env);
    expect(res.status).toBe(200);
    const body = (await res.json()) as { status: string };
    expect(body.status).toBe('actioned');
  });

  it('tolerates notes: null (CLI compatibility)', async () => {
    await seedArtifact('art_null');
    await seedReport({
      id: 'r_null',
      artifactId: 'art_null',
      receivedAt: 1_700_000_550,
    });
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(
      JSON.stringify({ action: 'no_action', notes: null }),
    );
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/report/r_null/action',
      body: bodyBytes,
    });
    const res = await app.fetch(
      mkReq('POST', '/v1/admin/report/r_null/action', bodyBytes, jws),
      env,
    );
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as { status: string; action_notes?: string };
    expect(body.status).toBe('reviewed');
    expect(body.action_notes).toBeUndefined();
  });

  it('rerun on actioned report → Admin.NoOp', async () => {
    await seedArtifact('art_c');
    await seedReport({
      id: 'r_c',
      artifactId: 'art_c',
      receivedAt: 1_700_000_400,
    });
    const app = mkApp();
    // First action.
    const b1 = new TextEncoder().encode(JSON.stringify({ action: 'no_action' }));
    const s1 = await signJws({
      method: 'POST',
      path: '/v1/admin/report/r_c/action',
      body: b1,
    });
    let res = await app.fetch(mkReq('POST', '/v1/admin/report/r_c/action', b1, s1.jws), env);
    expect(res.status).toBe(200);
    // Second action (rerun).
    const b2 = new TextEncoder().encode(JSON.stringify({ action: 'removed' }));
    const s2 = await signJws({
      method: 'POST',
      path: '/v1/admin/report/r_c/action',
      body: b2,
    });
    res = await app.fetch(mkReq('POST', '/v1/admin/report/r_c/action', b2, s2.jws), env);
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Admin');
    expect(j.detail).toBe('NoOp');
  });

  it('invalid action string → Malformed.BadRequest', async () => {
    await seedArtifact('art_d');
    await seedReport({
      id: 'r_d',
      artifactId: 'art_d',
      receivedAt: 1_700_000_500,
    });
    const app = mkApp();
    const bodyBytes = new TextEncoder().encode(JSON.stringify({ action: 'nuke_site' }));
    const { jws } = await signJws({
      method: 'POST',
      path: '/v1/admin/report/r_d/action',
      body: bodyBytes,
    });
    const res = await app.fetch(mkReq('POST', '/v1/admin/report/r_d/action', bodyBytes, jws), env);
    expect(res.status).toBe(400);
    const j = (await res.json()) as { kind: string; detail: string };
    expect(j.kind).toBe('Malformed');
    expect(j.detail).toBe('BadRequest');
  });
});
