import { describe, it, expect } from 'vitest';
import * as ed from '@noble/ed25519';
import { SELF } from 'cloudflare:test';
import { isModerator } from '../src/lib/moderator';
import { encodeCursor, decodeCursor, type Cursor } from '../src/lib/cursor';
import { parseMultipart, MultipartError } from '../src/lib/multipart';
import type { Env } from '../src/env';
import { loadWasm } from '../src/lib/wasm';
import { signJws } from './helpers/signer';

/**
 * Covers the three small utilities owned by W2T8:
 *  - moderator allowlist (case-insensitive, whitespace-tolerant)
 *  - opaque cursor round-trip
 *  - multipart/form-data parsing via native Request.formData()
 *
 * Also hosts the shared `uploadFixture` helper used by `upload.test.ts`'s
 * display_name coverage (Task 5 of the 2026-04-26 identity-completion plan).
 * Lives here per plan §T5 owner-files list — utils.test.ts is the single
 * worker-test home for cross-file Tier-B helpers.
 */

function mkEnv(pubkeys: string): Env {
  // Only `OMNI_ADMIN_PUBKEYS` is read by isModerator; cast to satisfy the
  // full Env shape without materializing R2/D1/KV/DO bindings in tests.
  return { OMNI_ADMIN_PUBKEYS: pubkeys } as unknown as Env;
}

describe('isModerator', () => {
  const K = 'aa'.repeat(32); // 64-hex dummy pubkey
  const K2 = 'bb'.repeat(32);

  it('returns true for an exact match', () => {
    expect(isModerator(K, mkEnv(K))).toBe(true);
  });

  it('matches case-insensitively', () => {
    expect(isModerator(K.toUpperCase(), mkEnv(K))).toBe(true);
    expect(isModerator(K, mkEnv(K.toUpperCase()))).toBe(true);
  });

  it('tolerates whitespace and empty entries', () => {
    const raw = `  ,${K.toUpperCase()}  ,, ${K2} ,`;
    expect(isModerator(K, mkEnv(raw))).toBe(true);
    expect(isModerator(K2, mkEnv(raw))).toBe(true);
  });

  it('returns false when not listed, empty, or undefined', () => {
    expect(isModerator(K, mkEnv(''))).toBe(false);
    expect(isModerator(K, mkEnv(K2))).toBe(false);
    expect(isModerator('', mkEnv(K))).toBe(false);
    // Env with OMNI_ADMIN_PUBKEYS undefined (e.g. pre-binding local tests).
    expect(isModerator(K, {} as unknown as Env)).toBe(false);
  });
});

describe('cursor encode/decode', () => {
  const shapes: Cursor[] = [
    { t: 0, i: '' },
    { t: 1712345678901, i: '01HX7QJ5Y0ABCD' },
    { t: '2026-04-14T00:00:00Z', i: 'id-with-dashes' },
    { t: 'tag:unicode-☃', i: 'row/42' },
    { t: Number.MAX_SAFE_INTEGER, i: 'x'.repeat(64) },
  ];

  for (const c of shapes) {
    it(`round-trips ${JSON.stringify(c)}`, () => {
      const s = encodeCursor(c);
      // base64url: no +, /, =.
      expect(s).not.toMatch(/[+/=]/);
      expect(decodeCursor(s)).toEqual(c);
    });
  }

  it('rejects malformed payload shape', () => {
    const bogus = encodeCursor({ t: 1, i: 'x' }).replace(/./, ''); // truncate
    expect(() => decodeCursor(bogus)).toThrow();
  });
});

describe('parseMultipart', () => {
  function mkReq(fd: FormData): Request {
    return new Request('https://example.invalid/upload', {
      method: 'POST',
      body: fd,
    });
  }

  it('returns Uint8Arrays for bundle + thumbnail when both present', async () => {
    const fd = new FormData();
    const bundleBytes = new Uint8Array([0x50, 0x4b, 0x03, 0x04]); // PK\x03\x04
    const thumbBytes = new Uint8Array([0x52, 0x49, 0x46, 0x46]); // RIFF
    fd.append('bundle', new Blob([bundleBytes], { type: 'application/zip' }), 'theme.omni');
    fd.append('thumbnail', new Blob([thumbBytes], { type: 'image/webp' }), 'thumb.webp');

    const parts = await parseMultipart(mkReq(fd));
    expect(parts.bundle).toBeInstanceOf(Uint8Array);
    expect(parts.thumbnail).toBeInstanceOf(Uint8Array);
    expect(Array.from(parts.bundle)).toEqual(Array.from(bundleBytes));
    expect(Array.from(parts.thumbnail)).toEqual(Array.from(thumbBytes));
  });

  it('throws MultipartError when bundle is missing', async () => {
    const fd = new FormData();
    fd.append('thumbnail', new Blob([new Uint8Array([1, 2, 3])], { type: 'image/webp' }), 't.webp');
    await expect(parseMultipart(mkReq(fd))).rejects.toBeInstanceOf(MultipartError);
  });

  it('throws MultipartError when thumbnail is missing', async () => {
    const fd = new FormData();
    fd.append(
      'bundle',
      new Blob([new Uint8Array([1, 2, 3])], { type: 'application/zip' }),
      't.omni',
    );
    await expect(parseMultipart(mkReq(fd))).rejects.toBeInstanceOf(MultipartError);
  });

  it('throws MultipartError when a part is a string instead of a file', async () => {
    const fd = new FormData();
    fd.append('bundle', 'not-a-file');
    fd.append('thumbnail', new Blob([new Uint8Array([1])], { type: 'image/webp' }), 't.webp');
    await expect(parseMultipart(mkReq(fd))).rejects.toBeInstanceOf(MultipartError);
  });
});

// ---------------------------------------------------------------------------
// uploadFixture — shared Tier-B upload helper.
// ---------------------------------------------------------------------------
//
// Builds a signed bundle inline (the workerd vitest pool can't read disk
// fixtures — see do.test.ts), wraps it in multipart with optional extra form
// fields, signs the request envelope, and returns `{ status, pubkey }` so
// callers can assert on `authors.pubkey = ?` rows after the upload.
//
// Why here: plan §T5 explicitly names `apps/worker/test/utils.test.ts` as
// the helper's home so every test suite touching uploads can import the
// same shape. The function is exported (vitest tolerates non-test exports
// in `*.test.ts` files; describe blocks above still execute as normal).

/** Standard fixture device-fingerprint (matches upload.test.ts / e2e_roundtrip.test.ts). */
const FIXTURE_DF_HEX = 'dc9773ca5d79ecfdedf0c8cca1cfecac9bc39c09550aec75a8cbe8b2a13b67a1';

/**
 * Default fixture seed (matches upload.test.ts / e2e_roundtrip.test.ts /
 * fixtures.json so the JWS oracle bytes line up byte-for-byte). Any
 * `uploadFixture` call without an explicit `seed` uses this identity.
 */
const FIXTURE_SEED_HEX = '0707070707070707070707070707070707070707070707070707070707070707';

/**
 * Tiny valid PNG (8x8 grayscale). Identical bytes to upload.test.ts /
 * e2e_roundtrip.test.ts so the thumbnail SHA-256 is reproducible across
 * suites if anything ever wants to assert on it.
 */
const FIXTURE_THUMB_PNG = new Uint8Array([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
  0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
  0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x62, 0x00, 0x01, 0x00, 0x00,
  0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
  0x42, 0x60, 0x82,
]);

const OVERLAY_BYTES = new TextEncoder().encode(
  '<widget><template><div data-sensor="cpu.usage"/></template></widget>',
);
const THEME_CSS_BYTES = new TextEncoder().encode(
  '/* omni test */\nbody { background: #111; color: #eee; }\n',
);

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

async function buildFixtureSignedBundle(
  seed: Uint8Array,
  opts: { name?: string; tags?: string[] } = {},
): Promise<Uint8Array> {
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
    description: 'inline fixture for uploadFixture',
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
  return identity.packSignedBundle(manifest, filesMap, seed, undefined);
}

/**
 * Build a multipart/form-data body. Always includes `bundle` + `thumbnail`;
 * extra string fields (e.g. `display_name`) are appended in iteration order.
 *
 * Hand-rolled rather than using `FormData` because we need byte-stable bytes
 * for JWS body_sha256 and `FormData.toString()` doesn't exist in workerd.
 */
function buildFixtureMultipart(
  bundle: Uint8Array,
  thumbnail: Uint8Array,
  extraFields: Record<string, string> = {},
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

  for (const [key, value] of Object.entries(extraFields)) {
    parts.push(enc.encode(`\r\n--${boundary}\r\n`));
    parts.push(enc.encode(`Content-Disposition: form-data; name="${key}"\r\n\r\n`));
    parts.push(enc.encode(value));
  }

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

export interface UploadFixtureOptions {
  /** Worker env (only used to make rate-limit reset visible if callers need it; currently unused but kept for future-proofing per plan §T5). */
  env?: Env;
  /**
   * Optional 32-byte Ed25519 seed; defaults to FIXTURE_SEED_HEX. The pubkey
   * derived from this seed is the JWS `kid` AND the bundle's author identity.
   */
  seed?: Uint8Array;
  /** Optional bundle-name override (drives manifest.name and per-author name uniqueness). */
  bundleName?: string;
  /**
   * Optional `display_name` multipart form field. `undefined` means the field
   * is not appended (absent from the body); a string is appended verbatim
   * (no client-side validation — that's the worker's job).
   */
  displayName?: string;
}

export interface UploadFixtureResult {
  status: number;
  /** Public key (32 bytes) derived from the seed. The authors-row pubkey for this upload. */
  pubkey: Uint8Array;
  /** Public key as lowercase hex (64 chars). */
  pubkeyHex: string;
  /** The raw Response so callers can `.json()` / `.text()` for further assertions. */
  response: Response;
}

/**
 * Drives a `/v1/upload` request end-to-end: builds a signed bundle, packages
 * a multipart envelope (with optional `display_name` field), signs the JWS
 * envelope, and dispatches via SELF.fetch.
 *
 * Used by upload.test.ts for the display_name coverage block; matches the
 * shape called out in plan §T5 step 5.1.
 */
export async function uploadFixture(opts: UploadFixtureOptions = {}): Promise<UploadFixtureResult> {
  const seed = opts.seed ?? hexToBytes(FIXTURE_SEED_HEX);
  const pubkey = await ed.getPublicKeyAsync(seed);
  const df = hexToBytes(FIXTURE_DF_HEX);

  const bundle = await buildFixtureSignedBundle(seed, { name: opts.bundleName });

  const extraFields: Record<string, string> = {};
  if (opts.displayName !== undefined) {
    extraFields.display_name = opts.displayName;
  }

  const { body, contentType } = buildFixtureMultipart(bundle, FIXTURE_THUMB_PNG, extraFields);
  const path = '/v1/upload';
  const method = 'POST';
  const jws = await signJws({ method, path, body, seed, pubkey, df });

  const response = await SELF.fetch(`https://worker.test${path}`, {
    method,
    headers: {
      Authorization: `Omni-JWS ${jws}`,
      'Content-Type': contentType,
      'X-Omni-Version': '0.1.0',
      'X-Omni-Sanitize-Version': '1',
    },
    body,
  });

  return {
    status: response.status,
    pubkey,
    pubkeyHex: bytesToHex(pubkey),
    response,
  };
}
