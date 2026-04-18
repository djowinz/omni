/**
 * Tier B — Miniflare-backed integration tests for POST /v1/upload (W3T10).
 *
 * Coverage (spec §14 + plan):
 *   - happy-path (theme-only bundle, inline-minted)
 *   - dedup → status "deduplicated"
 *   - name-conflict → CONFLICT
 *   - tampered bundle → Integrity/Unsafe
 *   - unknown tag → MANIFEST_INVALID with suggested_alternatives
 *   - SIZE_EXCEEDED (413)
 *   - missing Authorization → AUTH_MALFORMED_ENVELOPE
 *   - pubkey mismatch → FORBIDDEN
 *   - rate-limited after quota pre-fill
 *
 * Bundles are built inline via `loadWasm().identity.packSignedBundle` because
 * `node:fs` is not available inside the workers isolate (see do.test.ts).
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { env, SELF, applyD1Migrations } from 'cloudflare:test';
import * as ed from '@noble/ed25519';
import type { Env } from '../src/env';
import { loadWasm } from '../src/lib/wasm';
import { signJws } from './helpers/signer';

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

// ---- Fixture key material (matches fixtures.json / W1T3) ------------------
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

const OVERLAY_BYTES = new TextEncoder().encode(
  '<overlay><template><div data-sensor="cpu.usage"/></template></overlay>',
);
const THEME_CSS_BYTES = new TextEncoder().encode(
  '/* omni test */\nbody { background: #111; color: #eee; }\n',
);

interface BundleOpts {
  name?: string;
  tags?: string[];
}

async function buildSignedBundle(opts: BundleOpts = {}): Promise<Uint8Array> {
  const { identity } = await loadWasm();
  const entries = [
    { path: 'overlay.omni', bytes: OVERLAY_BYTES },
    { path: 'themes/default.css', bytes: THEME_CSS_BYTES },
  ];
  const manifest: Record<string, unknown> = {
    schema_version: 1,
    name: opts.name ?? 'upload-test',
    version: '1.0.0',
    omni_min_version: '0.1.0',
    description: 'inline fixture for upload.test.ts',
    tags: opts.tags ?? [],
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
  return identity.packSignedBundle(manifest, filesMap, SEED, undefined);
}

// ---- Multipart helper ------------------------------------------------------
function buildMultipart(
  bundle: Uint8Array,
  thumbnail: Uint8Array,
): {
  body: Uint8Array;
  contentType: string;
} {
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

const TINY_PNG = new Uint8Array([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
  0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
  0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x62, 0x00, 0x01, 0x00, 0x00,
  0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
  0x42, 0x60, 0x82,
]);

async function uploadReq(
  bundle: Uint8Array,
  opts: {
    seed?: Uint8Array;
    pubkey?: Uint8Array;
    path?: string;
    method?: string;
  } = {},
): Promise<Response> {
  const { body, contentType } = buildMultipart(bundle, TINY_PNG);
  const path = opts.path ?? '/v1/upload';
  const method = opts.method ?? 'POST';
  const jws = await signJws({
    method,
    path,
    body,
    seed: opts.seed ?? SEED,
    pubkey: opts.pubkey ?? PUBKEY,
    df: DF,
  });
  return SELF.fetch(`https://worker.test${path}`, {
    method,
    headers: {
      Authorization: `Omni-JWS ${jws}`,
      'Content-Type': contentType,
      'X-Omni-Version': '0.1.0',
      'X-Omni-Sanitize-Version': '1',
    },
    body,
  });
}

// ---- Environment reset -----------------------------------------------------
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

// ---- Tests -----------------------------------------------------------------

describe('POST /v1/upload — happy path', () => {
  it('accepts signed theme-only bundle, returns status=created', async () => {
    const bundle = await buildSignedBundle();
    const res = await uploadReq(bundle);
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as {
      artifact_id: string;
      content_hash: string;
      status: string;
    };
    expect(body.status).toBe('created');
    expect(body.artifact_id.length).toBeGreaterThan(0);
    expect(body.content_hash).toMatch(/^[0-9a-f]{64}$/);
  });
});

describe('POST /v1/upload — dedup', () => {
  it('second upload of same content returns status=deduplicated', async () => {
    const bundle = await buildSignedBundle();
    const r1 = await uploadReq(bundle);
    expect(r1.status).toBe(200);
    const r2 = await uploadReq(bundle);
    expect(r2.status).toBe(200);
    const body = (await r2.json()) as { status: string };
    expect(body.status).toBe('deduplicated');
  });
});

describe('POST /v1/upload — name conflict', () => {
  it('rejects different content with same (author, name) as CONFLICT', async () => {
    const first = await buildSignedBundle({ name: 'collide' });
    const r1 = await uploadReq(first);
    expect(r1.status, await r1.clone().text()).toBe(200);

    // Defeat the dedup short-circuit by wiping content_hashes (step 10 miss),
    // then submit a different bundle with the same name.
    await env.META.exec('DELETE FROM content_hashes');

    // Second bundle with same name but new content: vary description.
    // packSignedBundle is deterministic for the same manifest+files, so we
    // vary by using a second CSS file.
    const { identity } = await loadWasm();
    const entries = [
      { path: 'overlay.omni', bytes: OVERLAY_BYTES },
      { path: 'themes/default.css', bytes: new TextEncoder().encode('/* v2 */body{}') },
    ];
    const manifest2 = {
      schema_version: 1,
      name: 'collide',
      version: '1.0.0',
      omni_min_version: '0.1.0',
      description: 'v2',
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
    const second = identity.packSignedBundle(manifest2, filesMap, SEED, undefined);

    const r2 = await uploadReq(second);
    expect(r2.status).toBe(409);
    const body = (await r2.json()) as { error: { code: string } };
    expect(body.error.code).toBe('CONFLICT');
  });
});

describe('POST /v1/upload — tampered bundle', () => {
  it('rejects with Integrity or Unsafe kind (from unpackSignedBundle)', async () => {
    const valid = await buildSignedBundle();
    const tampered = new Uint8Array(valid);
    // Flip one byte before the central directory (same pattern as do.test.ts).
    let cdOffset = -1;
    for (let i = tampered.length - 4; i >= 0; i--) {
      if (
        tampered[i] === 0x50 &&
        tampered[i + 1] === 0x4b &&
        tampered[i + 2] === 0x01 &&
        tampered[i + 3] === 0x02
      ) {
        cdOffset = i;
        break;
      }
    }
    expect(cdOffset).toBeGreaterThan(64);
    tampered[cdOffset - 32] ^= 0xff;

    const res = await uploadReq(tampered);
    expect([400, 422]).toContain(res.status);
    const body = (await res.json()) as { kind?: string };
    expect(['Integrity', 'Unsafe', 'Malformed']).toContain(body.kind);
  });
});

describe('POST /v1/upload — unknown tag', () => {
  it('returns MANIFEST_INVALID with suggested_alternatives (Levenshtein-1)', async () => {
    await seedVocab(['dark', 'minimal']);
    const bundle = await buildSignedBundle({ tags: ['darkk'] });
    const res = await uploadReq(bundle);
    expect(res.status).toBe(400);
    const body = (await res.json()) as {
      error: { code: string };
      kind?: string;
      detail?: string;
      suggested_alternatives?: string[];
    };
    expect(body.error.code).toBe('MANIFEST_INVALID');
    expect(body.detail).toBe('UnknownTag');
    expect(body.suggested_alternatives).toContain('dark');
  });
});

describe('POST /v1/upload — SIZE_EXCEEDED', () => {
  it('returns 413 when body exceeds max_bundle_compressed (via default cache)', async () => {
    // The module caches config:limits for 60s per isolate. We can't reliably
    // reseed a tiny cap mid-isolate; instead we pad well past the DEFAULT
    // seeded 5 MiB cap (this test file's beforeEach sets 5 MiB) so the cap is
    // tripped even if the cache is warm.
    const oversize = new Uint8Array(6 * 1024 * 1024); // 6 MiB > 5 MiB cap
    const { body, contentType } = buildMultipart(oversize, TINY_PNG);
    // JWS body_sha256 binds to `body`; still sign it so the size guard can
    // trip BEFORE auth (step 1). With oversize the route returns 413 without
    // verifying the JWS.
    const jws = await signJws({
      method: 'POST',
      path: '/v1/upload',
      body,
      seed: SEED,
      pubkey: PUBKEY,
      df: DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/upload', {
      method: 'POST',
      headers: {
        Authorization: `Omni-JWS ${jws}`,
        'Content-Type': contentType,
        'X-Omni-Version': '0.1.0',
        'X-Omni-Sanitize-Version': '1',
      },
      body,
    });
    expect(res.status).toBe(413);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('SIZE_EXCEEDED');
  });
});

describe('POST /v1/upload — auth failures', () => {
  it('missing Authorization → 401 AUTH_MALFORMED_ENVELOPE', async () => {
    const bundle = await buildSignedBundle();
    const { body, contentType } = buildMultipart(bundle, TINY_PNG);
    const res = await SELF.fetch('https://worker.test/v1/upload', {
      method: 'POST',
      headers: {
        'Content-Type': contentType,
        'X-Omni-Version': '0.1.0',
        'X-Omni-Sanitize-Version': '1',
      },
      body,
    });
    expect(res.status).toBe(401);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('AUTH_MALFORMED_ENVELOPE');
  });
});

describe('POST /v1/upload — pubkey mismatch → FORBIDDEN', () => {
  it('403 when JWS kid differs from bundle author pubkey', async () => {
    // Bundle signed by fixture SEED (PUBKEY_HEX). Sign the JWS with a
    // *different* keypair — the JWS verifies cleanly, but its kid won't
    // match SignedBundle.authorPubkey() → FORBIDDEN.
    const otherSeed = new Uint8Array(32).fill(0x11);
    const otherPub = await ed.getPublicKeyAsync(otherSeed);
    const bundle = await buildSignedBundle();
    const res = await uploadReq(bundle, { seed: otherSeed, pubkey: otherPub });
    expect(res.status).toBe(403);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('FORBIDDEN');
  });
});

describe('POST /v1/upload — rate limited', () => {
  it('returns 429 RATE_LIMITED when quota is pre-filled', async () => {
    const day = new Date().toISOString().slice(0, 10);
    await env.STATE.put(`quota:device:${DF_HEX}:${day}`, String(1_000_000), { expirationTtl: 120 });
    const bundle = await buildSignedBundle();
    const res = await uploadReq(bundle);
    expect(res.status).toBe(429);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('RATE_LIMITED');
  });
});
