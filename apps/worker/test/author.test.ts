import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { env, SELF, applyD1Migrations } from 'cloudflare:test';
import type { Env } from '../src/env';
import { signJws as signJwsShared } from './helpers/signer';

/**
 * Tier B - Miniflare-backed tests for the identity-completion spec section 4.2:
 * `GET /v1/author/:pubkey_hex` (public) and `PUT /v1/author/me` (JWS-auth).
 *
 * GET returns AuthorDetail (pubkey_hex, fingerprint_hex, display_name,
 * joined_at, total_uploads) or 404. PUT requires a JWS-signed envelope and
 * upserts `authors.display_name` per the section 3.4 validation rules
 * (NFC + trim + 1..=32 + control/surrogate reject, no banlist v1).
 */

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

// ---------------------------------------------------------------------------
// Fixture key material - same convention used by report.test.ts /
// admin_artifact.test.ts so the JWS oracle bytes line up.
// ---------------------------------------------------------------------------
const SEED_HEX = '0707070707070707070707070707070707070707070707070707070707070707';
const PUBKEY_HEX = 'ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c';
const DF_HEX = 'dc9773ca5d79ecfdedf0c8cca1cfecac9bc39c09550aec75a8cbe8b2a13b67a1';

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
}

const SEED = hexToBytes(SEED_HEX);
const PUBKEY_BYTES = hexToBytes(PUBKEY_HEX);
const DF_BYTES = hexToBytes(DF_HEX);

interface SignOptions {
  method: string;
  path: string;
  body?: Uint8Array | string;
  query?: string;
  ts?: number;
  sanitizeVersion?: number;
}

async function signJws(o: SignOptions): Promise<string> {
  return signJwsShared({
    method: o.method,
    path: o.path,
    body: o.body,
    query: o.query,
    seed: SEED,
    pubkey: PUBKEY_BYTES,
    df: DF_BYTES,
    ts: o.ts,
    sanitizeVersion: o.sanitizeVersion,
  });
}

// ---------------------------------------------------------------------------
// D1 schema seeding via the committed migrations.
// ---------------------------------------------------------------------------

beforeAll(async () => {
  // Apply the committed migration files so post-0002 schema (display_name has
  // no UNIQUE) is in effect. Same pattern as report.test.ts /
  // admin_reports.test.ts: listMigrations() is the supported path; if the
  // pool variant doesn't expose it (or returns nothing), fall back to an
  // inline CREATE so tests still get a usable authors table.
  const migrations = await import('cloudflare:test').then((m) =>
    (m as unknown as { listMigrations?: () => Promise<unknown> }).listMigrations?.(),
  );
  if (migrations) {
    await applyD1Migrations(
      env.META,
      migrations as unknown as Parameters<typeof applyD1Migrations>[1],
    );
  } else {
    // Fallback inline schema. Use the post-0002 shape (no UNIQUE on
    // display_name) so the rest of the test bodies match production after
    // the spec's migration has run.
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS authors (pubkey BLOB PRIMARY KEY, display_name TEXT, created_at INTEGER NOT NULL, total_uploads INTEGER NOT NULL DEFAULT 0, is_new_creator INTEGER NOT NULL DEFAULT 1, is_denied INTEGER NOT NULL DEFAULT 0)`,
    );
  }
});

beforeEach(async () => {
  // Fresh authors table per test so display_name / total_uploads assertions
  // are deterministic. Other tables stay (reports/artifacts) - none touched
  // by this route.
  await env.META.exec('DELETE FROM authors');
});

// Standard headers required by the global X-Omni-Version gate in
// `apps/worker/src/index.ts`. The gate exempts `/v1/config/*`, GETs to
// `/v1/download/*` and `/v1/thumbnail/*` only - `/v1/author/*` does NOT
// have an exemption, so every request (auth'd or not) carries these.
const STD_HEADERS: Record<string, string> = {
  'X-Omni-Version': '0.1.0',
  'X-Omni-Sanitize-Version': '1',
};

// ---------------------------------------------------------------------------
// GET /v1/author/:pubkey_hex
// ---------------------------------------------------------------------------

describe('GET /v1/author/:pubkey_hex', () => {
  it('returns 404 when no row exists', async () => {
    const res = await SELF.fetch(`https://example.com/v1/author/${PUBKEY_HEX}`, {
      headers: STD_HEADERS,
    });
    expect(res.status).toBe(404);
  });

  it('returns author detail for a known pubkey', async () => {
    await env.META.prepare(
      'INSERT INTO authors (pubkey, display_name, created_at, total_uploads) VALUES (?, ?, ?, ?)',
    )
      .bind(PUBKEY_BYTES, 'starfire', 1_714_000_000, 7)
      .run();

    const res = await SELF.fetch(`https://example.com/v1/author/${PUBKEY_HEX}`, {
      headers: STD_HEADERS,
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as Record<string, unknown>;
    expect(body).toMatchObject({
      pubkey_hex: PUBKEY_HEX,
      display_name: 'starfire',
      joined_at: 1_714_000_000,
      total_uploads: 7,
    });
    // fingerprint_hex = first 6 bytes of SHA-256(pubkey), 12 hex chars.
    expect(body.fingerprint_hex).toMatch(/^[0-9a-f]{12}$/);
  });

  it('returns 400 for malformed pubkey_hex (not 64-hex)', async () => {
    const res = await SELF.fetch('https://example.com/v1/author/notvalidhex', {
      headers: STD_HEADERS,
    });
    expect(res.status).toBe(400);
  });

  it('returns null display_name when authors row has no name yet', async () => {
    await env.META.prepare(
      'INSERT INTO authors (pubkey, display_name, created_at) VALUES (?, NULL, ?)',
    )
      .bind(PUBKEY_BYTES, 1_714_000_000)
      .run();

    const res = await SELF.fetch(`https://example.com/v1/author/${PUBKEY_HEX}`, {
      headers: STD_HEADERS,
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { display_name: unknown };
    expect(body.display_name).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// PUT /v1/author/me - JWS-authenticated
// ---------------------------------------------------------------------------

describe('PUT /v1/author/me', () => {
  it('rejects unsigned request with 401', async () => {
    const res = await SELF.fetch('https://example.com/v1/author/me', {
      method: 'PUT',
      headers: { ...STD_HEADERS, 'Content-Type': 'application/json' },
      body: JSON.stringify({ display_name: 'starfire' }),
    });
    expect(res.status).toBe(401);
  });

  it('upserts display_name on signed PUT', async () => {
    const bodyStr = JSON.stringify({ display_name: 'starfire' });
    const jws = await signJws({
      method: 'PUT',
      path: '/v1/author/me',
      body: bodyStr,
    });

    const res = await SELF.fetch('https://example.com/v1/author/me', {
      method: 'PUT',
      headers: {
        ...STD_HEADERS,
        Authorization: `Omni-JWS ${jws}`,
        'Content-Type': 'application/json',
      },
      body: bodyStr,
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { display_name: string; pubkey_hex: string };
    expect(body.display_name).toBe('starfire');
    expect(body.pubkey_hex).toBe(PUBKEY_HEX);

    // Verify on disk.
    const row = await env.META.prepare('SELECT display_name FROM authors WHERE pubkey = ?')
      .bind(PUBKEY_BYTES)
      .first<{ display_name: string }>();
    expect(row?.display_name).toBe('starfire');
  });

  it('rejects empty (whitespace-only) display_name with 400', async () => {
    const bodyStr = JSON.stringify({ display_name: '   ' });
    const jws = await signJws({ method: 'PUT', path: '/v1/author/me', body: bodyStr });
    const res = await SELF.fetch('https://example.com/v1/author/me', {
      method: 'PUT',
      headers: {
        ...STD_HEADERS,
        Authorization: `Omni-JWS ${jws}`,
        'Content-Type': 'application/json',
      },
      body: bodyStr,
    });
    expect(res.status).toBe(400);
  });

  it('rejects display_name longer than 32 chars after trim', async () => {
    const long = 'x'.repeat(33);
    const bodyStr = JSON.stringify({ display_name: long });
    const jws = await signJws({ method: 'PUT', path: '/v1/author/me', body: bodyStr });
    const res = await SELF.fetch('https://example.com/v1/author/me', {
      method: 'PUT',
      headers: {
        ...STD_HEADERS,
        Authorization: `Omni-JWS ${jws}`,
        'Content-Type': 'application/json',
      },
      body: bodyStr,
    });
    expect(res.status).toBe(400);
  });

  // Regression — T4 quality review I1 carry-forward: validateDisplayName must
  // measure length in Unicode CODE POINTS (matching the host Rust validator's
  // s.chars().count()), NOT UTF-16 code units. An astral-plane emoji like 😀
  // (U+1F600) counts as 1 code point but 2 UTF-16 code units. Pre-fix, mixing
  // ASCII + emoji at the 32-cap boundary was accepted by Rust and rejected by
  // the worker. Below: 24 code points (15 'a' + 9 '😀') = 33 UTF-16 units.
  it('accepts 24-code-point input that has 33 UTF-16 code units (emoji boundary)', async () => {
    const value = 'a'.repeat(15) + '😀'.repeat(9);
    expect([...value].length).toBe(24); // 24 code points (≤32, accept)
    expect(value.length).toBe(33); // 33 UTF-16 units (pre-fix would reject)

    const bodyStr = JSON.stringify({ display_name: value });
    const jws = await signJws({ method: 'PUT', path: '/v1/author/me', body: bodyStr });
    const res = await SELF.fetch('https://example.com/v1/author/me', {
      method: 'PUT',
      headers: {
        ...STD_HEADERS,
        Authorization: `Omni-JWS ${jws}`,
        'Content-Type': 'application/json',
      },
      body: bodyStr,
    });
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as { display_name: string };
    expect(body.display_name).toBe(value);
  });

  it('rejects 33-code-point input regardless of unit choice', async () => {
    // 'a'.repeat(33) is 33 code points AND 33 UTF-16 units; both old + new
    // implementations reject. Locks in that the cap is enforced at 32.
    const value = 'a'.repeat(33);
    expect([...value].length).toBe(33);
    const bodyStr = JSON.stringify({ display_name: value });
    const jws = await signJws({ method: 'PUT', path: '/v1/author/me', body: bodyStr });
    const res = await SELF.fetch('https://example.com/v1/author/me', {
      method: 'PUT',
      headers: {
        ...STD_HEADERS,
        Authorization: `Omni-JWS ${jws}`,
        'Content-Type': 'application/json',
      },
      body: bodyStr,
    });
    expect(res.status).toBe(400);
  });

  it('rejects control characters (NUL byte) with 400', async () => {
    // U+0000 is in \p{Cc} - must reject per spec section 3.4 step 4.
    const bodyStr = JSON.stringify({ display_name: 'star\x00fire' });
    const jws = await signJws({ method: 'PUT', path: '/v1/author/me', body: bodyStr });
    const res = await SELF.fetch('https://example.com/v1/author/me', {
      method: 'PUT',
      headers: {
        ...STD_HEADERS,
        Authorization: `Omni-JWS ${jws}`,
        'Content-Type': 'application/json',
      },
      body: bodyStr,
    });
    expect(res.status).toBe(400);
  });

  it('NFC-normalizes input (decomposed combining acute composes to single code point)', async () => {
    // Build the decomposed (NFD) form deterministically with explicit
    // code-points: 'starfir' + U+0065 ('e') + U+0301 (combining acute) = 9
    // code units. After NFC, the trailing e + combining-acute composes to
    // a single U+00E9 (precomposed e-acute), giving 'starfir' + U+00E9 = 8.
    const nfd = 'starfir' + String.fromCodePoint(0x0065) + String.fromCodePoint(0x0301);
    expect(nfd.length).toBe(9);

    const bodyStr = JSON.stringify({ display_name: nfd });
    const jws = await signJws({ method: 'PUT', path: '/v1/author/me', body: bodyStr });
    const res = await SELF.fetch('https://example.com/v1/author/me', {
      method: 'PUT',
      headers: {
        ...STD_HEADERS,
        Authorization: `Omni-JWS ${jws}`,
        'Content-Type': 'application/json',
      },
      body: bodyStr,
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as { display_name: string };
    // Canonical NFC composed form: 'starfir' + U+00E9 = 8 code units.
    const expected = 'starfir' + String.fromCodePoint(0x00e9);
    expect(body.display_name).toBe(expected);
    expect(body.display_name.length).toBe(8);
  });
});
