/**
 * Round-trip integration test for the `AuthorNameConflict` recovery envelope
 * on POST /v1/upload Step 11 (per-author name uniqueness).
 *
 * Spec §8.7 / INV-7.6.3 / Plan Task A2.5:
 *   This is the SECOND test for the AuthorNameConflict envelope — a companion
 *   to `upload-author-name-conflict.test.ts` (Task A0.5-6). Where the basic
 *   test seeds D1 directly to assert envelope existence and field-shape, this
 *   round-trip test exercises the full publish pipeline TWICE — first an
 *   actual successful publish, then a conflicting re-publish — and asserts
 *   the structured `detail` payload faithfully relays the FIRST publish's
 *   `artifact_id`, `version`, and `last_published_at` back to the renderer's
 *   Step 4 amber recovery card. End-to-end data-flow coverage, not just shape.
 *
 * Why the round-trip matters separately:
 *   The basic A0.5-6 test stubs the prior artifact in D1 with a hand-picked
 *   `id` / `version` / `updated_at`, so it can't catch a regression where the
 *   route reads any of those columns from the wrong source (e.g., the new
 *   request's manifest, the JWS claims, or a stale cache). This test catches
 *   that class of bug because the asserted values are the actual first
 *   publish's response.
 *
 * Harness pattern mirrors `apps/worker/test/upload.test.ts` and
 * `apps/worker/test/upload-author-name-conflict.test.ts` (Tier B
 * miniflare-backed integration). Bundles are inline-minted via
 * `loadWasm().identity.packSignedBundle`.
 *
 * Pre-existing-failure note (per plan handoff):
 *   The legacy `upload.test.ts` uses `<overlay>...</overlay>` as the top
 *   element, which the current sanitize-DO rejects (8 pre-existing failures
 *   on this branch — independent regression). This file uses `<widget>` to
 *   stay hermetic against that drift; the existing
 *   `upload-author-name-conflict.test.ts` does the same.
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { env, SELF, applyD1Migrations } from 'cloudflare:test';
import type { Env } from '../src/env';
import { loadWasm } from '../src/lib/wasm';
import { signJws } from './helpers/signer';

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

// ---- Fixture key material (matches upload.test.ts) -----------------------
const SEED_HEX = '0707070707070707070707070707070707070707070707070707070707070707';
const PUBKEY_HEX = 'ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c';
const DF_HEX = 'dc9773ca5d79ecfdedf0c8cca1cfecac9bc39c09550aec75a8cbe8b2a13b67a1';

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}
function bytesToHex(b: Uint8Array): string {
  let s = '';
  for (let i = 0; i < b.length; i++) s += b[i]!.toString(16).padStart(2, '0');
  return s;
}
async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const d = await crypto.subtle.digest('SHA-256', bytes);
  return bytesToHex(new Uint8Array(d));
}

const SEED = hexToBytes(SEED_HEX);
const PUBKEY = hexToBytes(PUBKEY_HEX);
const DF = hexToBytes(DF_HEX);

// `<widget>` per the canonical sanitize fixture (see file-header note about
// `<overlay>` legacy drift).
const OVERLAY_BYTES = new TextEncoder().encode(
  '<widget><template><div data-sensor="cpu.usage"/></template></widget>',
);

interface MintOpts {
  name: string;
  themeBytes: Uint8Array;
  version?: string;
  description?: string;
}

async function mintBundle(opts: MintOpts): Promise<Uint8Array> {
  const { identity } = await loadWasm();
  const entries = [
    { path: 'overlay.omni', bytes: OVERLAY_BYTES },
    { path: 'themes/default.css', bytes: opts.themeBytes },
  ];
  const manifest: Record<string, unknown> = {
    schema_version: 1,
    name: opts.name,
    version: opts.version ?? '1.0.0',
    omni_min_version: '0.1.0',
    description: opts.description ?? 'fixture',
    tags: [],
    license: 'MIT',
    entry_overlay: 'overlay.omni',
    default_theme: 'themes/default.css',
    sensor_requirements: [],
    files: await Promise.all(
      entries.map(async (f) => ({ path: f.path, sha256: await sha256Hex(f.bytes) })),
    ),
    // Plain object (not Map) → `isThemeOnly` returns true, so both publishes
    // are billed against `upload_new` (5/day) and stay well under the limit.
    resource_kinds: {
      theme: { dir: 'themes/', extensions: ['.css'], max_size_bytes: 1_048_576 },
    },
  };
  const filesMap = new Map(entries.map((f) => [f.path, f.bytes] as const));
  return identity.packSignedBundle(manifest, filesMap, SEED, undefined);
}

// Tiny 1×1 PNG (matches upload.test.ts).
const TINY_PNG = new Uint8Array([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
  0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
  0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x62, 0x00, 0x01, 0x00, 0x00,
  0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
  0x42, 0x60, 0x82,
]);

function buildMultipart(
  bundle: Uint8Array,
  thumbnail: Uint8Array,
): { body: Uint8Array; contentType: string } {
  const boundary = '----omni-test-' + Math.random().toString(36).slice(2);
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
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.byteLength;
  }
  return { body: out, contentType: `multipart/form-data; boundary=${boundary}` };
}

async function uploadReq(bundle: Uint8Array): Promise<Response> {
  const { body, contentType } = buildMultipart(bundle, TINY_PNG);
  const path = '/v1/upload';
  const jws = await signJws({ method: 'POST', path, body, seed: SEED, pubkey: PUBKEY, df: DF });
  return SELF.fetch(`https://worker.test${path}`, {
    method: 'POST',
    headers: {
      Authorization: `Omni-JWS ${jws}`,
      'Content-Type': contentType,
      'X-Omni-Version': '0.1.0',
      'X-Omni-Sanitize-Version': '1',
    },
    body,
  });
}

async function resetEnv() {
  for (const prefix of ['quota:', 'denylist:', 'df_pubkey_velocity:']) {
    const list = await env.STATE.list({ prefix });
    for (const k of list.keys) await env.STATE.delete(k.name);
  }
  await env.META.exec('DELETE FROM artifacts');
  await env.META.exec('DELETE FROM content_hashes');
  await env.META.exec('DELETE FROM authors');
  await env.META.exec('DELETE FROM tombstones');
  await env.META.exec('DELETE FROM install_daily');
}

async function seedVocab(tags: string[] = []) {
  await env.STATE.put('config:vocab', JSON.stringify({ tags, version: 1 }));
}
async function seedLimits(maxCompressed = 5_242_880) {
  await env.STATE.put(
    'config:limits',
    JSON.stringify({
      max_bundle_compressed: maxCompressed,
      max_bundle_uncompressed: 10_485_760,
      max_entries: 32,
      version: 1,
      updated_at: 0,
    }),
  );
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
      `CREATE TABLE IF NOT EXISTS content_hashes (content_hash TEXT PRIMARY KEY, artifact_id TEXT NOT NULL, first_seen_at INTEGER NOT NULL)`,
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS tombstones (content_hash TEXT PRIMARY KEY, reason TEXT, removed_at INTEGER NOT NULL)`,
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS install_daily (artifact_id TEXT NOT NULL, day TEXT NOT NULL, install_count INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (artifact_id, day))`,
    );
  }
});

beforeEach(async () => {
  await resetEnv();
  await seedVocab([]);
  await seedLimits();
});

describe('POST /v1/upload — AuthorNameConflict round-trip recovery envelope', () => {
  it('relays first publish artifact_id + version + last_published_at on conflicting re-publish', async () => {
    // ---- First publish: real, end-to-end ---------------------------------
    const first = await mintBundle({
      name: 'marathon-hud',
      themeBytes: new TextEncoder().encode('/* v1 */body{background:#111;color:#eee}'),
      version: '1.0.0',
      description: 'first publish',
    });
    const r1 = await uploadReq(first);
    expect(r1.status, await r1.clone().text()).toBe(200);

    const r1Body = (await r1.json()) as {
      artifact_id: string;
      content_hash: string;
      created_at: number;
      status: string;
    };
    expect(r1Body.status).toBe('created');
    expect(r1Body.artifact_id.length).toBeGreaterThan(0);

    // The route returns `created_at` (epoch seconds). On a fresh `created`
    // row, `updated_at` == `created_at`, so the recovery envelope's
    // `last_published_at` (ISO of `updated_at`) must derive from this same
    // epoch second.
    const expectedLastPublishedIso = new Date(r1Body.created_at * 1000).toISOString();
    const expectedExistingId = r1Body.artifact_id;
    const expectedExistingVersion = '1.0.0';

    // ---- Defeat dedup short-circuit (Step 10 in the route) ---------------
    // Without this, packSignedBundle's deterministic packing would re-hash
    // identical content; we need the SECOND publish to reach Step 11
    // (per-author name conflict). Wiping `content_hashes` is the same
    // mechanism `upload.test.ts`'s name-conflict case uses.
    await env.META.exec('DELETE FROM content_hashes');

    // ---- Second publish: same name, different content --------------------
    const second = await mintBundle({
      name: 'marathon-hud',
      themeBytes: new TextEncoder().encode('/* v2 */body{background:#222;color:#fff}'),
      version: '1.0.1',
      description: 'second publish',
    });
    const r2 = await uploadReq(second);
    expect(r2.status, await r2.clone().text()).toBe(409);

    const r2Body = (await r2.json()) as {
      error: { code: string; message: string };
      kind?: string;
      detail?: string;
    };

    // Envelope shape (matches what the renderer's recovery card parses).
    expect(r2Body.error.code).toBe('AuthorNameConflict');
    expect(r2Body.error.message).toBe('Name already taken under your identity');
    expect(r2Body.kind).toBe('Malformed');
    expect(typeof r2Body.detail).toBe('string');

    // Detail payload — every field traces back to the FIRST publish's
    // response (round-trip data-flow assertion).
    const detail = JSON.parse(r2Body.detail!) as {
      existing_artifact_id: string;
      existing_version: string;
      last_published_at: string;
    };
    expect(detail.existing_artifact_id).toBe(expectedExistingId);
    expect(detail.existing_version).toBe(expectedExistingVersion);
    expect(detail.last_published_at).toBe(expectedLastPublishedIso);
  });
});
