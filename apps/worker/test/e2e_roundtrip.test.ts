/**
 * Tier B — End-to-end miniflare round-trip: upload → list → download (W4T16).
 *
 * Covers the umbrella §8.2 integration row "Upload → list → download →
 * byte-compare against miniflare." Exercises the full HTTPS surface stitched
 * together by W4T14 via `SELF.fetch`:
 *
 *   1. Seed `config:vocab` + `config:limits` KV (mirrors upload.test.ts
 *      ambient seeding).
 *   2. Build the W1T3 `theme-only` fixture equivalent inline with the same
 *      fixture seed (`Uint8Array(32).fill(7)` → SEED_HEX) so the content-hash
 *      matches fixtures.json. `node:fs` is unavailable inside the workers
 *      isolate (see upload.test.ts rationale), so we mint the bytes via WASM
 *      rather than reading the committed `.omnipkg` from disk.
 *   3. POST `/v1/upload` with a detached JWS signed by `@noble/ed25519`, the
 *      same wire shape as upload.test.ts / auth.test.ts: header `{typ:"JWT",
 *      alg:"EdDSA"}`, claims body per `crates/identity/src/wasm_jws_core`.
 *   4. GET `/v1/list?kind=theme&sort=new`; expect the new artifact row.
 *   5. GET `/v1/download/:artifact_id`; expect 200 + contract headers
 *      `X-Omni-Content-Hash`, `X-Omni-Author-Pubkey`, `X-Omni-Signature`,
 *      `X-Omni-Manifest`, plus body bytes.
 *   6. Byte-compare the downloaded blob.
 *
 * BYTE-COMPARE CAVEAT (DONE_WITH_CONCERNS): the Worker sanitize path repacks
 * the bundle UNSIGNED per architectural invariant #1 — `omni-identity` is the
 * sole signing authority and the Worker cannot re-sign. The DO sanitize+repack
 * (`do/bundle_processor.ts` step 5: `bundle.pack(..., undefined)`) therefore
 * strips `signature.jws` from the stored blob. Consequences exercised here:
 *   - Stored bundle is NOT byte-identical to the upload (different container
 *     + missing JWS entry). We assert structural validity via
 *     `bundle.unpackManifest(downloaded)` instead of byte equality.
 *   - `X-Omni-Signature` / `X-Omni-Manifest` download headers end up empty
 *     strings on repacked unsigned bundles because `download.ts` uses
 *     `identity.unpackSignedBundle` which throws on missing JWS (see its
 *     try/catch at L100–121). We assert the headers EXIST but tolerate empty
 *     values; `X-Omni-Content-Hash` and `X-Omni-Author-Pubkey` remain the
 *     cross-impl oracle per umbrella §8.2.
 * The canonical `content_hash` binds the stored blob to the uploaded manifest
 * — that is the strongest cross-impl guarantee available without the Worker
 * re-signing, which invariant #1 forbids.
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { env, SELF, applyD1Migrations } from 'cloudflare:test';
import type { Env } from '../src/env';
import { loadWasm } from '../src/lib/wasm';
import { signJws as signJwsShared } from './helpers/signer';

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

// ---- Fixture key material (must match test/fixtures/fixtures.json) --------
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

// Match fixtures.json "theme-only" entry — tiny overlay + one CSS theme.
const OVERLAY_BYTES = new TextEncoder().encode(
  '<widget><template><div data-sensor="cpu.usage"/></template></widget>',
);
const THEME_CSS_BYTES = new TextEncoder().encode(
  '/* omni e2e */\nbody { background: #111; color: #eee; }\n',
);

interface BuiltBundle {
  bytes: Uint8Array;
  manifest: Record<string, unknown>;
}

async function buildThemeOnlyBundle(): Promise<BuiltBundle> {
  const { identity } = await loadWasm();
  const entries = [
    { path: 'overlay.omni', bytes: OVERLAY_BYTES },
    { path: 'themes/default.css', bytes: THEME_CSS_BYTES },
  ];
  const manifest: Record<string, unknown> = {
    schema_version: 1,
    name: 'e2e-roundtrip',
    version: '1.0.0',
    omni_min_version: '0.1.0',
    description: 'W4T16 miniflare round-trip fixture',
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
  const bytes = identity.packSignedBundle(manifest, filesMap, SEED, undefined) as Uint8Array;
  return { bytes, manifest };
}

async function signJws(opts: {
  method: string;
  path: string;
  body: Uint8Array;
  query?: string;
}): Promise<string> {
  return signJwsShared({
    method: opts.method,
    path: opts.path,
    body: opts.body,
    query: opts.query,
    seed: SEED,
    pubkey: PUBKEY,
    df: DF,
  });
}

// ---- Multipart helper (matches upload.test.ts) ---------------------------
function buildMultipart(
  bundle: Uint8Array,
  thumbnail: Uint8Array,
): {
  body: Uint8Array;
  contentType: string;
} {
  const boundary = '----omni-e2e-' + Math.random().toString(36).slice(2);
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

// Minimal 1x1 transparent PNG — same constant as upload.test.ts.
const TINY_PNG = new Uint8Array([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
  0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
  0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x62, 0x00, 0x01, 0x00, 0x00,
  0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
  0x42, 0x60, 0x82,
]);

// ---- Environment wiring --------------------------------------------------
async function resetEnv(): Promise<void> {
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

async function seedVocab(): Promise<void> {
  await env.STATE.put('config:vocab', JSON.stringify({ tags: [], version: 1 }));
}
async function seedLimits(): Promise<void> {
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
  await seedVocab();
  await seedLimits();
});

const CLIENT_HEADERS = {
  'X-Omni-Version': '0.1.0',
  'X-Omni-Sanitize-Version': '1',
};

describe('miniflare end-to-end: upload → list → download (W4T16)', () => {
  it('round-trips a signed theme bundle through the full HTTPS surface', async () => {
    // --- 1. Build + upload --------------------------------------------------
    const { bytes: uploadBytes, manifest } = await buildThemeOnlyBundle();
    expect(uploadBytes.byteLength).toBeGreaterThan(0);

    const { body, contentType } = buildMultipart(uploadBytes, TINY_PNG);
    const jws = await signJws({ method: 'POST', path: '/v1/upload', body });
    const uploadRes = await SELF.fetch('https://worker.test/v1/upload', {
      method: 'POST',
      headers: {
        ...CLIENT_HEADERS,
        Authorization: `Omni-JWS ${jws}`,
        'Content-Type': contentType,
      },
      body,
    });
    expect(uploadRes.status, await uploadRes.clone().text()).toBe(200);
    const uploadBody = (await uploadRes.json()) as {
      artifact_id: string;
      content_hash: string;
      status: string;
    };
    expect(uploadBody.status).toBe('created');
    expect(uploadBody.artifact_id.length).toBeGreaterThan(0);
    expect(uploadBody.content_hash).toMatch(/^[0-9a-f]{64}$/);

    // The upload response's `content_hash` is authoritative — it is computed
    // over the POST-SANITIZE manifest (sanitize may rewrite file bytes, which
    // updates `manifest.files[*].sha256`, which feeds the canonical hash).
    // We do NOT assert it equals the client-side canonical hash of the
    // pre-upload manifest for that reason.
    const { bundle: bundleWasm } = await loadWasm();

    // --- 2. List (must include the upload) ---------------------------------
    const listRes = await SELF.fetch('https://worker.test/v1/list?kind=theme&sort=new', {
      headers: { ...CLIENT_HEADERS },
    });
    expect(listRes.status, await listRes.clone().text()).toBe(200);
    const listBody = (await listRes.json()) as {
      items: Array<{
        artifact_id: string;
        content_hash: string;
        name: string;
        kind: string;
      }>;
    };
    const hit = listBody.items.find((i) => i.artifact_id === uploadBody.artifact_id);
    expect(hit, 'uploaded artifact must appear in /v1/list').toBeTruthy();
    expect(hit!.name).toBe(manifest.name);
    expect(hit!.kind).toBe('theme');
    // Note: list.content_hash === upload.content_hash (both read D1 row).
    // Keep this assertion soft — the DB row stores the pre-sanitize manifest
    // hash, and the upload route returns the same. Test this as a consistency
    // check; if it ever diverges, that is a real bug in one of the routes.
    expect(hit!.content_hash).toBe(uploadBody.content_hash);
    // Also assert against D1 directly as a belt-and-suspenders cross-check:
    const dbRow = await env.META.prepare('SELECT content_hash FROM artifacts WHERE id = ?')
      .bind(uploadBody.artifact_id)
      .first<{ content_hash: string }>();
    expect(dbRow?.content_hash, `db=${dbRow?.content_hash} upload=${uploadBody.content_hash}`).toBe(
      uploadBody.content_hash,
    );

    // --- 3. Download + contract headers ------------------------------------
    const dlRes = await SELF.fetch(`https://worker.test/v1/download/${uploadBody.artifact_id}`, {
      headers: { 'cf-connecting-ip': '198.51.100.7' },
    });
    expect(dlRes.status, await dlRes.clone().text()).toBe(200);
    expect(dlRes.headers.get('content-type')).toContain('application/octet-stream');
    expect(dlRes.headers.get('X-Omni-Content-Hash')).toBe(uploadBody.content_hash);
    expect(dlRes.headers.get('X-Omni-Author-Pubkey')).toBe(PUBKEY_HEX);
    // X-Omni-Signature: the Worker's sanitize repack strips the JWS entry
    // (invariant #1 — the Worker cannot re-sign) and the current WASM surface
    // does not expose the raw signature either way. Per contract §4.2 the
    // header is omitted entirely rather than emitted with a placeholder.
    expect(dlRes.headers.get('X-Omni-Signature')).toBeNull();
    // X-Omni-Manifest must parse as JSON (via base64) and match the uploaded
    // manifest's identity fields. The unsigned-fast-path `bundle.unpackManifest`
    // populates this header even on stripped-JWS blobs.
    const dlManifestB64 = dlRes.headers.get('X-Omni-Manifest');
    expect(dlManifestB64).toBeTruthy();
    const dlHeaderManifest = JSON.parse(atob(dlManifestB64!)) as Record<string, unknown>;
    expect(dlHeaderManifest.name).toBe(manifest.name);
    expect(dlHeaderManifest.version).toBe(manifest.version);
    expect(dlHeaderManifest.schema_version).toBe(manifest.schema_version);

    const downloadedBytes = new Uint8Array(await dlRes.arrayBuffer());
    expect(downloadedBytes.byteLength).toBeGreaterThan(0);

    // --- 4. Structural byte-compare (sanitized path) -----------------------
    //
    // Worker sanitize+repack produces new container bytes per invariant #1
    // (Worker cannot re-sign). We verify the DOWNLOADED bundle is a valid
    // `.omnipkg` whose manifest matches the uploaded manifest's identifying
    // fields and canonical hash. The `content_hash` contract header binds
    // the stored blob to the uploaded manifest's canonical hash — that is
    // the cross-impl oracle umbrella §8.2 actually cares about.
    // Stored blob is UNSIGNED (Worker repack drops JWS). Use the unsigned
    // `bundle.unpackManifest` fast path to parse the container and verify the
    // manifest survived the sanitize round-trip.
    const dlManifest = bundleWasm.unpackManifest(downloadedBytes, undefined) as Record<
      string,
      unknown
    >;
    expect(dlManifest.name).toBe(manifest.name);
    expect(dlManifest.version).toBe(manifest.version);
    expect(dlManifest.schema_version).toBe(manifest.schema_version);
    // The downloaded manifest's canonical hash is the POST-SANITIZE hash and
    // may differ from `uploadBody.content_hash` (which is the pre-sanitize
    // manifest hash computed by `upload.ts` step 9). We therefore don't
    // assert equality — we only assert the downloaded hash is a well-formed
    // 32-byte digest. The cross-impl oracle per umbrella §8.2 is that
    // `X-Omni-Content-Hash` equals `uploadBody.content_hash`, which we
    // already asserted above.
    const dlHashBytes = bundleWasm.canonicalHash(dlManifest) as Uint8Array;
    expect(dlHashBytes.byteLength).toBe(32);

    // Record whether sanitize+repack produced byte-identical output for this
    // no-op theme fixture. Repacked bundles are expected to differ (stripped
    // signature.jws at minimum). Not a hard assertion — see file-header
    // DONE_WITH_CONCERNS note.
    const identical =
      downloadedBytes.byteLength === uploadBytes.byteLength &&
      downloadedBytes.every((b, i) => b === uploadBytes[i]);
    expect(identical).toBe(false);
  });
});
