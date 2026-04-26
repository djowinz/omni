/**
 * Tier B — Miniflare-backed integration tests for /v1/artifact/:id (W3T11).
 *
 * Covers spec #008 §8 / §9 / §10 + contract §4.4 / §4.5 / §4.6:
 *   - GET 404 on unknown id
 *   - GET public view does NOT include `reports`
 *   - GET with non-moderator JWS does NOT include `reports`
 *   - GET with moderator JWS includes `reports`
 *   - PATCH FORBIDDEN when JWS kid != author_pubkey
 *   - DELETE FORBIDDEN likewise
 *   - DELETE happy path → 204, row is marked is_removed=1
 *   - PATCH "unchanged" return when DO reports same canonical_hash
 *
 * PATCH/DELETE auth is author-only (contract §4.5/§4.6). PATCH exercises the
 * forward-to-DO path; the DO itself is a 501 stub until W3T9 lands, so we
 * only assert the pre-DO gates (auth/ownership/rate-limit) in this file.
 * Full end-to-end PATCH happy-path lives in the W4 integration suite.
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { env, SELF as RAW_SELF } from 'cloudflare:test';
import { loadWasm } from '../src/lib/wasm';

// Inject the global client-version headers (W4T14) into every Miniflare
// request unless the test already set them.
const SELF = {
  fetch(input: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    if (!headers.has('X-Omni-Version')) headers.set('X-Omni-Version', '0.1.0');
    if (!headers.has('X-Omni-Sanitize-Version')) headers.set('X-Omni-Sanitize-Version', '1');
    return RAW_SELF.fetch(input, { ...init, headers });
  },
};
import * as ed from '@noble/ed25519';
import type { Env } from '../src/env';
import { signJws } from './helpers/signer';

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

async function ensureSchema(): Promise<void> {
  await env.META.exec(
    `CREATE TABLE IF NOT EXISTS authors (pubkey BLOB PRIMARY KEY, display_name TEXT UNIQUE, created_at INTEGER NOT NULL, total_uploads INTEGER NOT NULL DEFAULT 0, is_new_creator INTEGER NOT NULL DEFAULT 1, is_denied INTEGER NOT NULL DEFAULT 0)`,
  );
  await env.META.exec(
    `CREATE TABLE IF NOT EXISTS artifacts (id TEXT PRIMARY KEY, author_pubkey BLOB NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, content_hash TEXT NOT NULL, thumbnail_hash TEXT NOT NULL, description TEXT, tags TEXT, license TEXT, version TEXT NOT NULL, omni_min_version TEXT NOT NULL, signature BLOB NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, install_count INTEGER NOT NULL DEFAULT 0, report_count INTEGER NOT NULL DEFAULT 0, is_removed INTEGER NOT NULL DEFAULT 0, is_featured INTEGER NOT NULL DEFAULT 0, UNIQUE (author_pubkey, name))`,
  );
  await env.META.exec(
    `CREATE TABLE IF NOT EXISTS content_hashes (content_hash TEXT PRIMARY KEY, artifact_id TEXT NOT NULL, first_seen_at INTEGER NOT NULL)`,
  );
}

// ---------- helpers ----------

function hexEncode(b: Uint8Array): string {
  let s = '';
  for (let i = 0; i < b.length; i++) s += b[i]!.toString(16).padStart(2, '0');
  return s;
}

async function sha256Hex(data: ArrayBuffer | Uint8Array): Promise<string> {
  const buf = data instanceof Uint8Array ? data : new Uint8Array(data);
  const d = await crypto.subtle.digest('SHA-256', buf);
  return hexEncode(new Uint8Array(d));
}

// ---------- fixtures ----------

const AUTHOR_SEED = new Uint8Array(32).fill(0x22);
let AUTHOR_PUB: Uint8Array;
const AUTHOR_DF = new Uint8Array(32).fill(0x33);

const OTHER_SEED = new Uint8Array(32).fill(0x44);
let OTHER_PUB: Uint8Array;
const OTHER_DF = new Uint8Array(32).fill(0x55);

const MOD_SEED = new Uint8Array(32).fill(0x66);
let MOD_PUB: Uint8Array;
const MOD_DF = new Uint8Array(32).fill(0x77);

async function resetD1(): Promise<void> {
  await env.META.exec('DELETE FROM artifacts');
  await env.META.exec('DELETE FROM content_hashes');
  await env.META.exec('DELETE FROM authors');
}

async function resetKv(): Promise<void> {
  for (const prefix of ['quota:', 'df_pubkey_velocity:', 'denylist:']) {
    const l = await env.STATE.list({ prefix });
    for (const k of l.keys) await env.STATE.delete(k.name);
  }
}

async function seedArtifact(opts: {
  id: string;
  authorPub: Uint8Array;
  name?: string;
  tags?: string[];
  reports?: number;
}): Promise<void> {
  const name = opts.name ?? `name-${opts.id}`;
  const tags = JSON.stringify(opts.tags ?? []);
  await env.META.prepare('INSERT OR IGNORE INTO authors (pubkey, created_at) VALUES (?, 1000)')
    .bind(opts.authorPub)
    .run();
  await env.META.prepare(
    `INSERT INTO artifacts (
       id, author_pubkey, name, kind, content_hash, thumbnail_hash,
       description, tags, license, version, omni_min_version, signature,
       created_at, updated_at, install_count, report_count
     ) VALUES (?, ?, ?, 'theme', ?, ?, '', ?, 'MIT', '1.0.0', '0.1.0',
               X'00', 1000, 1000, 0, ?)`,
  )
    .bind(
      opts.id,
      opts.authorPub,
      name,
      `hash_${opts.id}`,
      `thumb_${opts.id}`,
      tags,
      opts.reports ?? 7,
    )
    .run();
}

beforeAll(async () => {
  await ensureSchema();
});

beforeEach(async () => {
  await resetD1();
  await resetKv();
  // PATCH routes config:limits through to the DO; fail-closed if unseeded.
  await env.STATE.put(
    'config:limits',
    JSON.stringify({
      max_bundle_compressed: 5_242_880,
      max_bundle_uncompressed: 10_485_760,
      max_entries: 32,
      version: 1,
      updated_at: 0,
    }),
  );
  AUTHOR_PUB ||= await ed.getPublicKeyAsync(AUTHOR_SEED);
  OTHER_PUB ||= await ed.getPublicKeyAsync(OTHER_SEED);
  MOD_PUB ||= await ed.getPublicKeyAsync(MOD_SEED);
});

// ---------- GET /v1/artifact/:id ----------

describe('GET /v1/artifact/:id — 404', () => {
  it('unknown id returns NOT_FOUND', async () => {
    const res = await SELF.fetch('https://worker.test/v1/artifact/missing');
    expect(res.status).toBe(404);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('NOT_FOUND');
  });
});

describe('GET /v1/artifact/:id — public view', () => {
  it('unauthenticated GET omits reports field', async () => {
    await seedArtifact({ id: 'art1', authorPub: AUTHOR_PUB, reports: 7 });
    const res = await SELF.fetch('https://worker.test/v1/artifact/art1');
    expect(res.status).toBe(200);
    const body = (await res.json()) as Record<string, unknown>;
    expect(body.artifact_id).toBe('art1');
    expect(body.author_pubkey).toBe(hexEncode(AUTHOR_PUB));
    expect(body.status).toBe('live');
    expect('reports' in body).toBe(false);
  });
});

describe('GET /v1/artifact/:id — authed non-moderator', () => {
  it('still omits reports', async () => {
    await seedArtifact({ id: 'art1', authorPub: AUTHOR_PUB, reports: 7 });
    const jws = await signJws({
      method: 'GET',
      path: '/v1/artifact/art1',
      seed: AUTHOR_SEED,
      pubkey: AUTHOR_PUB,
      df: AUTHOR_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/artifact/art1', {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as Record<string, unknown>;
    expect('reports' in body).toBe(false);
  });
});

describe('GET /v1/artifact/:id — moderator JWS', () => {
  it('includes reports field when kid is on moderator allowlist', async () => {
    await seedArtifact({ id: 'art1', authorPub: AUTHOR_PUB, reports: 7 });
    const modPubHex = hexEncode(MOD_PUB);
    const jws = await signJws({
      method: 'GET',
      path: '/v1/artifact/art1',
      seed: MOD_SEED,
      pubkey: MOD_PUB,
      df: MOD_DF,
    });
    // Use app.request() rather than SELF.fetch so we can pass an env override —
    // Miniflare's SELF request pipeline snapshots env before the test mutation
    // reaches it. app.request runs the Hono app in-process using the env arg
    // we pass.
    const app = (await import('../src/index')).default;
    const modEnv = { ...env, OMNI_ADMIN_PUBKEYS: modPubHex };
    const res = await app.request(
      '/v1/artifact/art1',
      {
        headers: {
          Authorization: `Omni-JWS ${jws}`,
          'X-Omni-Version': '0.1.0',
          'X-Omni-Sanitize-Version': '1',
        },
      },
      modEnv,
    );
    expect(res.status).toBe(200);
    const body = (await res.json()) as Record<string, unknown>;
    expect(body.reports).toBe(7);
  });
});

describe('GET /v1/artifact/:id — tombstoned status', () => {
  it('status=tombstoned when is_removed=1', async () => {
    await seedArtifact({ id: 'art1', authorPub: AUTHOR_PUB });
    await env.META.prepare('UPDATE artifacts SET is_removed=1 WHERE id=?').bind('art1').run();
    const res = await SELF.fetch('https://worker.test/v1/artifact/art1');
    const body = (await res.json()) as { status: string };
    expect(body.status).toBe('tombstoned');
  });
});

// ---------- PATCH /v1/artifact/:id ----------

describe('PATCH /v1/artifact/:id — FORBIDDEN for non-author', () => {
  it('403 when JWS kid is not the author_pubkey', async () => {
    await seedArtifact({ id: 'art1', authorPub: AUTHOR_PUB });
    const jws = await signJws({
      method: 'PATCH',
      path: '/v1/artifact/art1',
      seed: OTHER_SEED,
      pubkey: OTHER_PUB,
      df: OTHER_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/artifact/art1', {
      method: 'PATCH',
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(403);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('FORBIDDEN');
  });
});

describe('PATCH /v1/artifact/:id — missing auth', () => {
  it('401 AUTH_MALFORMED_ENVELOPE', async () => {
    await seedArtifact({ id: 'art1', authorPub: AUTHOR_PUB });
    const res = await SELF.fetch('https://worker.test/v1/artifact/art1', {
      method: 'PATCH',
    });
    expect(res.status).toBe(401);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('AUTH_MALFORMED_ENVELOPE');
  });
});

describe('PATCH /v1/artifact/:id — 404', () => {
  it('404 on unknown id even after auth passes', async () => {
    const jws = await signJws({
      method: 'PATCH',
      path: '/v1/artifact/missing',
      seed: AUTHOR_SEED,
      pubkey: AUTHOR_PUB,
      df: AUTHOR_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/artifact/missing', {
      method: 'PATCH',
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(404);
  });
});

// ---------- DELETE /v1/artifact/:id ----------

describe('DELETE /v1/artifact/:id — 204 on owner', () => {
  it('marks is_removed=1 and returns 204', async () => {
    await seedArtifact({ id: 'art1', authorPub: AUTHOR_PUB });
    const jws = await signJws({
      method: 'DELETE',
      path: '/v1/artifact/art1',
      seed: AUTHOR_SEED,
      pubkey: AUTHOR_PUB,
      df: AUTHOR_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/artifact/art1', {
      method: 'DELETE',
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(204);
    const row = await env.META.prepare('SELECT is_removed FROM artifacts WHERE id = ?')
      .bind('art1')
      .first<{ is_removed: number }>();
    expect(row?.is_removed).toBe(1);
  });
});

describe('DELETE /v1/artifact/:id — FORBIDDEN for non-author', () => {
  it('403 when JWS kid is not the author_pubkey', async () => {
    await seedArtifact({ id: 'art1', authorPub: AUTHOR_PUB });
    const jws = await signJws({
      method: 'DELETE',
      path: '/v1/artifact/art1',
      seed: OTHER_SEED,
      pubkey: OTHER_PUB,
      df: OTHER_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/artifact/art1', {
      method: 'DELETE',
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(403);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('FORBIDDEN');
  });
});

// ---------- fixtures for manifest-returning GET + end-to-end PATCH ----------

// Seed identity that matches W1T3 fixtures.json: building via
// `identity.packSignedBundle` with this SEED produces a bundle whose
// author_pubkey equals PUBKEY_HEX. The artifact test fixtures above use
// filler byte arrays (`AUTHOR_SEED = Uint8Array(32).fill(0x22)`) that are
// also valid ed25519 seeds — signJws works with any seed because the JWS
// verifier trusts the `kid` claim. For tests that also need a valid
// *bundle* (GET manifest + PATCH happy path), we switch to the fixture
// seed so the bundle's internal signature verifies via WASM.
const FIXTURE_SEED_HEX = '0707070707070707070707070707070707070707070707070707070707070707';
const FIXTURE_PUBKEY_HEX = 'ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c';
const FIXTURE_DF_HEX = 'dc9773ca5d79ecfdedf0c8cca1cfecac9bc39c09550aec75a8cbe8b2a13b67a1';

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}
const FIXTURE_SEED = hexToBytes(FIXTURE_SEED_HEX);
const FIXTURE_PUB = hexToBytes(FIXTURE_PUBKEY_HEX);
const FIXTURE_DF = hexToBytes(FIXTURE_DF_HEX);

// Minimal valid overlay: sanitize's TOP_LEVEL_ELEMENTS allowlist
// (crates/bundle/src/omni_schema.rs:19) accepts only <theme>, <config>,
// <widget>. Use <widget> per the Rust sanitize integration tests at
// crates/sanitize/tests/handler_overlay.rs.
const OVERLAY_BYTES = new TextEncoder().encode(
  '<widget><template><div data-sensor="cpu.usage"/></template></widget>',
);
const THEME_CSS_BYTES = new TextEncoder().encode(
  '/* omni artifact test */\nbody { background: #222; color: #fff; }\n',
);
const THEME_CSS_BYTES_ALT = new TextEncoder().encode(
  '/* omni artifact test — alt */\nbody { background: #333; color: #eee; }\n',
);

async function buildSignedBundle(opts: {
  name: string;
  version?: string;
  themeBytes?: Uint8Array;
}): Promise<{ bytes: Uint8Array; manifestName: string; version: string }> {
  const { identity } = await loadWasm();
  const themeBytes = opts.themeBytes ?? THEME_CSS_BYTES;
  const entries = [
    { path: 'overlay.omni', bytes: OVERLAY_BYTES },
    { path: 'themes/default.css', bytes: themeBytes },
  ];
  const version = opts.version ?? '1.0.0';
  const manifest: Record<string, unknown> = {
    schema_version: 1,
    name: opts.name,
    version,
    omni_min_version: '0.1.0',
    description: 'artifact-test fixture',
    tags: [],
    license: 'MIT',
    entry_overlay: 'overlay.omni',
    default_theme: 'themes/default.css',
    sensor_requirements: [],
    files: await Promise.all(
      entries.map(async (f) => ({ path: f.path, sha256: await sha256Hex(f.bytes) })),
    ),
    resource_kinds: {
      theme: { dir: 'themes/', extensions: ['.css'], max_size_bytes: 1_048_576 },
    },
  };
  const filesMap = new Map(entries.map((f) => [f.path, f.bytes] as const));
  const bytes = identity.packSignedBundle(
    manifest,
    filesMap,
    FIXTURE_SEED,
    undefined,
  ) as Uint8Array;
  return { bytes, manifestName: opts.name, version };
}

async function seedArtifactWithBlob(opts: {
  id: string;
  bundleBytes: Uint8Array;
  contentHash: string;
  authorPub: Uint8Array;
  name: string;
  version: string;
}): Promise<void> {
  await env.BLOBS.put(`bundles/${opts.contentHash}.omnipkg`, opts.bundleBytes);
  await env.META.prepare('INSERT OR IGNORE INTO authors (pubkey, created_at) VALUES (?, 1000)')
    .bind(opts.authorPub)
    .run();
  await env.META.prepare(
    `INSERT INTO artifacts (
       id, author_pubkey, name, kind, content_hash, thumbnail_hash,
       description, tags, license, version, omni_min_version, signature,
       created_at, updated_at, install_count, report_count
     ) VALUES (?, ?, ?, 'theme', ?, ?, '', '[]', 'MIT', ?, '0.1.0',
               X'00', 1000, 1000, 0, 0)`,
  )
    .bind(opts.id, opts.authorPub, opts.name, opts.contentHash, `thumb_${opts.id}`, opts.version)
    .run();
}

function buildMultipart(
  bundle: Uint8Array,
  thumbnail: Uint8Array,
): {
  body: Uint8Array;
  contentType: string;
} {
  const boundary = '----omni-artifact-test-' + Math.random().toString(36).slice(2);
  const enc = new TextEncoder();
  const parts: Uint8Array[] = [];
  parts.push(enc.encode(`--${boundary}\r\n`));
  parts.push(
    enc.encode(`Content-Disposition: form-data; name="bundle"; filename="bundle.omnipkg"\r\n`),
  );
  parts.push(enc.encode(`Content-Type: application/octet-stream\r\n\r\n`));
  parts.push(bundle);
  parts.push(enc.encode(`\r\n--${boundary}\r\n`));
  parts.push(
    enc.encode(`Content-Disposition: form-data; name="thumbnail"; filename="thumb.png"\r\n`),
  );
  parts.push(enc.encode(`Content-Type: image/png\r\n\r\n`));
  parts.push(thumbnail);
  parts.push(enc.encode(`\r\n--${boundary}--\r\n`));
  let total = 0;
  for (const p of parts) total += p.byteLength;
  const body = new Uint8Array(total);
  let off = 0;
  for (const p of parts) {
    body.set(p, off);
    off += p.byteLength;
  }
  return { body, contentType: `multipart/form-data; boundary=${boundary}` };
}

// Worker-side canonical hash: what the DO returns after sanitize+repack.
// We compute it via WASM so tests observe the same value the worker persists.
async function canonicalHashHex(bundleBytes: Uint8Array): Promise<string> {
  const { bundle } = await loadWasm();
  const manifest = bundle.unpackManifest(bundleBytes, undefined);
  const hash = bundle.canonicalHash(manifest) as Uint8Array;
  return hexEncode(hash);
}

describe('GET /v1/artifact/:id — manifest extraction', () => {
  it('returns manifest object extracted from R2 blob', async () => {
    const built = await buildSignedBundle({ name: 'get-manifest-test' });
    const contentHash = await canonicalHashHex(built.bytes);
    await seedArtifactWithBlob({
      id: 'artM',
      bundleBytes: built.bytes,
      contentHash,
      authorPub: FIXTURE_PUB,
      name: built.manifestName,
      version: built.version,
    });
    const res = await SELF.fetch('https://worker.test/v1/artifact/artM');
    expect(res.status).toBe(200);
    const body = (await res.json()) as { manifest: unknown; content_hash: string };
    expect(body.manifest).not.toBeNull();
    expect(typeof body.manifest).toBe('object');
    const m = body.manifest as Record<string, unknown>;
    expect(m.name).toBe('get-manifest-test');
    expect(m.version).toBe('1.0.0');
    expect(m.schema_version).toBe(1);
    expect(Array.isArray(m.files)).toBe(true);
  });

  it('returns manifest: null when R2 blob is missing', async () => {
    // seedArtifact (the existing helper) does NOT write to R2; confirm the
    // route degrades cleanly rather than 500ing.
    await seedArtifact({ id: 'artNoBlob', authorPub: FIXTURE_PUB });
    const res = await SELF.fetch('https://worker.test/v1/artifact/artNoBlob');
    expect(res.status).toBe(200);
    const body = (await res.json()) as { manifest: unknown };
    expect(body.manifest).toBeNull();
  });
});

describe('PATCH /v1/artifact/:id — end-to-end DO roundtrip', () => {
  it("returns {status:'unchanged'} when PATCHing the same bundle twice", async () => {
    // Seed row with a placeholder content_hash; first PATCH syncs it to the
    // DO's post-sanitize canonical_hash; second PATCH with the same bundle
    // should short-circuit to {status:"unchanged"}.
    const built = await buildSignedBundle({ name: 'patch-unchanged' });
    await seedArtifactWithBlob({
      id: 'artU',
      bundleBytes: built.bytes,
      contentHash: '0'.repeat(64),
      authorPub: FIXTURE_PUB,
      name: built.manifestName,
      version: built.version,
    });
    const mp = buildMultipart(built.bytes, new Uint8Array([0x89, 0x50, 0x4e, 0x47]));
    async function doPatch(): Promise<Response> {
      const jws = await signJws({
        method: 'PATCH',
        path: '/v1/artifact/artU',
        body: mp.body,
        seed: FIXTURE_SEED,
        pubkey: FIXTURE_PUB,
        df: FIXTURE_DF,
      });
      return SELF.fetch('https://worker.test/v1/artifact/artU', {
        method: 'PATCH',
        headers: {
          Authorization: `Omni-JWS ${jws}`,
          'content-type': mp.contentType,
        },
        body: mp.body,
      });
    }
    const first = await doPatch();
    expect(first.status).toBe(200);
    const firstBody = (await first.json()) as { status: string; content_hash: string };
    expect(firstBody.status).toBe('updated');
    const second = await doPatch();
    expect(second.status).toBe(200);
    const secondBody = (await second.json()) as { status: string; content_hash: string };
    expect(secondBody.status).toBe('unchanged');
    expect(secondBody.content_hash).toBe(firstBody.content_hash);
  });

  it("returns {status:'updated'} with new content_hash for a modified bundle", async () => {
    const original = await buildSignedBundle({ name: 'patch-updated' });
    const originalHash = await canonicalHashHex(original.bytes);
    await seedArtifactWithBlob({
      id: 'artUpd',
      bundleBytes: original.bytes,
      contentHash: originalHash,
      authorPub: FIXTURE_PUB,
      name: original.manifestName,
      version: original.version,
    });
    // Build a new bundle with the same name + author but different content.
    const updated = await buildSignedBundle({
      name: 'patch-updated',
      version: '1.1.0',
      themeBytes: THEME_CSS_BYTES_ALT,
    });
    const mp = buildMultipart(updated.bytes, new Uint8Array([0x89, 0x50, 0x4e, 0x47]));
    const jws = await signJws({
      method: 'PATCH',
      path: '/v1/artifact/artUpd',
      body: mp.body,
      seed: FIXTURE_SEED,
      pubkey: FIXTURE_PUB,
      df: FIXTURE_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/artifact/artUpd', {
      method: 'PATCH',
      headers: {
        Authorization: `Omni-JWS ${jws}`,
        'content-type': mp.contentType,
      },
      body: mp.body,
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      status: string;
      content_hash: string;
      artifact_id: string;
    };
    expect(body.status).toBe('updated');
    expect(body.artifact_id).toBe('artUpd');
    expect(body.content_hash).not.toBe(originalHash);
    // D1 row + R2 blob reflect the new hash.
    const row = await env.META.prepare('SELECT content_hash, version FROM artifacts WHERE id = ?')
      .bind('artUpd')
      .first<{ content_hash: string; version: string }>();
    expect(row?.content_hash).toBe(body.content_hash);
    expect(row?.version).toBe('1.1.0');
    const newBlob = await env.BLOBS.head(`bundles/${body.content_hash}.omnipkg`);
    expect(newBlob).not.toBeNull();
  });
});

describe('DELETE /v1/artifact/:id — 404', () => {
  it('404 on unknown id after auth passes', async () => {
    const jws = await signJws({
      method: 'DELETE',
      path: '/v1/artifact/nope',
      seed: AUTHOR_SEED,
      pubkey: AUTHOR_PUB,
      df: AUTHOR_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/artifact/nope', {
      method: 'DELETE',
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(404);
  });
});
