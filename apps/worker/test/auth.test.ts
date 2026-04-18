import { describe, it, expect, beforeEach } from 'vitest';
import { env } from 'cloudflare:test';
import * as ed from '@noble/ed25519';
import type { Env } from '../src/env';
import { verifyJws, AuthError } from '../src/lib/auth';

/**
 * Tier B — Miniflare-backed tests for the attached-payload JWS verifier
 * (`src/lib/auth.ts`). Every JWS fed to `verifyJws` is minted with
 * `@noble/ed25519` using the shipped `HttpJwsClaims` wire shape from
 * `crates/identity/src/http_jws.rs`:
 *
 *   signing_input = base64url(header_json) + '.' + base64url(claims_json)
 *   header_json   = {"typ":"Omni-HTTP-JWS","alg":"EdDSA"}
 *                   (jsonwebtoken::Header default field order: typ, alg)
 *   claims_json   = {"alg":"EdDSA","crv":"Ed25519","typ":"Omni-HTTP-JWS",
 *                    "kid":<std-b64 pubkey>,"df":<std-b64 device_fp>,
 *                    "ts":<i64>,"method":<UPPER>,"path":<"/...">,
 *                    "query_sha256":<hex|"">,"body_sha256":<hex|"">,
 *                    "sanitize_version":<u32>}
 *
 * `kid` / `df` use STANDARD base64 (`+/` alphabet, padding preserved), NOT
 * base64url and NOT hex. If the Rust oracle drifts, the parity regression in
 * `omni-identity` fires first.
 *
 * Fixture keypair comes from `test/fixtures/fixtures.json` (plan #008 W1T3).
 */

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

// ---------------------------------------------------------------------------
// Fixture key material (plan #008 W1T3 — seed 0x07 repeated, matching native).
// ---------------------------------------------------------------------------
const SEED_HEX = '0707070707070707070707070707070707070707070707070707070707070707';
const PUBKEY_HEX = 'ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c';
const DF_HEX = 'dc9773ca5d79ecfdedf0c8cca1cfecac9bc39c09550aec75a8cbe8b2a13b67a1';

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

const SEED = hexToBytes(SEED_HEX);
const PUBKEY_BYTES = hexToBytes(PUBKEY_HEX);
const DF_BYTES = hexToBytes(DF_HEX);

/** Standard base64 (RFC 4648 §4, `+/` alphabet, padding preserved). */
function b64StdEncode(bytes: Uint8Array): string {
  let bin = '';
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]!);
  return btoa(bin);
}

const B64URL_CHARS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';

function b64urlEncode(bytes: Uint8Array | string): string {
  const b = typeof bytes === 'string' ? new TextEncoder().encode(bytes) : bytes;
  let s = '';
  let i = 0;
  for (; i + 3 <= b.length; i += 3) {
    const n = (b[i]! << 16) | (b[i + 1]! << 8) | b[i + 2]!;
    s +=
      B64URL_CHARS[(n >> 18) & 63] +
      B64URL_CHARS[(n >> 12) & 63] +
      B64URL_CHARS[(n >> 6) & 63] +
      B64URL_CHARS[n & 63];
  }
  if (i < b.length) {
    const rem = b.length - i;
    const n = (b[i]! << 16) | ((rem > 1 ? b[i + 1]! : 0) << 8);
    s += B64URL_CHARS[(n >> 18) & 63] + B64URL_CHARS[(n >> 12) & 63];
    if (rem === 2) s += B64URL_CHARS[(n >> 6) & 63];
  }
  return s;
}

async function sha256Hex(data: ArrayBuffer | Uint8Array): Promise<string> {
  const buf = data instanceof Uint8Array ? data : new Uint8Array(data);
  const d = await crypto.subtle.digest('SHA-256', buf);
  return bytesToHex(new Uint8Array(d));
}

interface SignOptions {
  method?: string;
  path?: string;
  query?: string;
  body?: Uint8Array;
  ts?: number;
  sanitizeVersion?: number;
  /** Override pubkey bytes placed in `kid` (standard base64). Default: fixture pubkey. */
  pubkey?: Uint8Array;
  /** Override device fingerprint bytes placed in `df` (standard base64). Default: fixture df. */
  df?: Uint8Array;
  header?: string; // override header JSON verbatim (for alg tests)
  mutateSignature?: boolean;
}

/**
 * Mint a compact JWS byte-for-byte equivalent to the shipped `sign_http_jws`
 * oracle in `crates/identity/src/http_jws.rs`. Header is
 * `{"typ":"Omni-HTTP-JWS","alg":"EdDSA"}` (jsonwebtoken::Header default field
 * order). Claims are the 11 `HttpJwsClaims` fields in struct-declaration
 * order; `kid` / `df` are standard base64.
 */
async function signJws(o: SignOptions = {}): Promise<string> {
  const body = o.body ?? new Uint8Array();
  const query = o.query ?? '';
  const method = o.method ?? 'POST';
  const path = o.path ?? '/v1/upload';
  const ts = o.ts ?? Math.floor(Date.now() / 1000);
  const sanitizeVersion = o.sanitizeVersion ?? 1;
  const pubkey = o.pubkey ?? PUBKEY_BYTES;
  const dfBytes = o.df ?? DF_BYTES;

  const body_sha256 = await sha256Hex(body);
  const query_sha256 = await sha256Hex(new TextEncoder().encode(query));
  // Field order matches `HttpJwsClaims` struct declaration: alg, crv, typ,
  // kid, df, ts, method, path, query_sha256, body_sha256, sanitize_version.
  const claims = {
    alg: 'EdDSA',
    crv: 'Ed25519',
    typ: 'Omni-HTTP-JWS',
    kid: b64StdEncode(pubkey),
    df: b64StdEncode(dfBytes),
    ts,
    method,
    path,
    query_sha256,
    body_sha256,
    sanitize_version: sanitizeVersion,
  };
  // jsonwebtoken::Header default serialization: typ first, alg second
  // (struct declaration order with None-skipping serde).
  const headerJson = o.header ?? '{"typ":"Omni-HTTP-JWS","alg":"EdDSA"}';
  const claimsJson = JSON.stringify(claims);
  const headerB64 = b64urlEncode(headerJson);
  const payloadB64 = b64urlEncode(claimsJson);
  const signingInput = new TextEncoder().encode(`${headerB64}.${payloadB64}`);
  let sig = await ed.signAsync(signingInput, SEED);
  if (o.mutateSignature) {
    sig = new Uint8Array(sig);
    sig[0] = sig[0]! ^ 0xff;
  }
  return `${headerB64}.${payloadB64}.${b64urlEncode(sig)}`;
}

function mkReq(
  jws: string | null,
  opts: { method?: string; path?: string; body?: Uint8Array; query?: string } = {},
): Request {
  const method = opts.method ?? 'POST';
  const query = opts.query ?? '';
  const url = `https://worker.test${opts.path ?? '/v1/upload'}${query ? `?${query}` : ''}`;
  const headers = new Headers();
  if (jws !== null) headers.set('Authorization', `Omni-JWS ${jws}`);
  const init: RequestInit = { method, headers };
  if (opts.body !== undefined && method !== 'GET') {
    init.body = opts.body;
  }
  return new Request(url, init);
}

async function expectAuthError(p: Promise<unknown>, detail: string): Promise<AuthError> {
  try {
    await p;
  } catch (e) {
    expect(e).toBeInstanceOf(AuthError);
    const ae = e as AuthError;
    expect(ae.detail).toBe(detail);
    return ae;
  }
  throw new Error(`expected AuthError with detail ${detail}`);
}

beforeEach(async () => {
  // Clear per-test KV state that persists between tests in the same miniflare.
  await env.STATE.delete(`denylist:pubkey:${PUBKEY_HEX}`);
});

describe('verifyJws — happy path', () => {
  it('accepts a correctly-signed request and returns AuthedRequest', async () => {
    const body = new TextEncoder().encode('{"hello":"world"}');
    const jws = await signJws({ body });
    const req = mkReq(jws, { body });
    const auth = await verifyJws(req, env, body.buffer as ArrayBuffer);
    expect(bytesToHex(auth.pubkey)).toBe(PUBKEY_HEX);
    expect(bytesToHex(auth.device_fp)).toBe(DF_HEX);
    expect(auth.sanitize_version).toBe(1);
    expect(auth.ts).toBeGreaterThan(0);
  });

  it('accepts empty-body + empty-query GET-shaped request', async () => {
    const body = new Uint8Array();
    const jws = await signJws({ method: 'POST', path: '/v1/report', body });
    const req = mkReq(jws, { method: 'POST', path: '/v1/report', body });
    const auth = await verifyJws(req, env, body.buffer as ArrayBuffer);
    expect(bytesToHex(auth.pubkey)).toBe(PUBKEY_HEX);
  });

  it('accepts non-empty query when hashed into query_sha256', async () => {
    const body = new Uint8Array();
    const query = 'kind=bundle&sort=new';
    const jws = await signJws({ body, query, path: '/v1/list', method: 'GET' });
    const req = mkReq(jws, { method: 'GET', path: '/v1/list', query });
    const auth = await verifyJws(req, env, body.buffer as ArrayBuffer);
    expect(auth.sanitize_version).toBe(1);
  });
});

describe('verifyJws — envelope failures', () => {
  it('missing Authorization → AUTH_MALFORMED_ENVELOPE', async () => {
    const req = mkReq(null);
    const ae = await expectAuthError(verifyJws(req, env, new ArrayBuffer(0)), 'MalformedEnvelope');
    expect(ae.code).toBe('AUTH_MALFORMED_ENVELOPE');
  });

  it('wrong prefix (Bearer) → AUTH_MALFORMED_ENVELOPE', async () => {
    const body = new Uint8Array();
    const jws = await signJws({ body });
    const req = new Request('https://worker.test/v1/upload', {
      method: 'POST',
      headers: { Authorization: `Bearer ${jws}` },
    });
    await expectAuthError(verifyJws(req, env, body.buffer as ArrayBuffer), 'MalformedEnvelope');
  });

  it('non-3-segment compact → AUTH_MALFORMED_ENVELOPE', async () => {
    const req = new Request('https://worker.test/v1/upload', {
      method: 'POST',
      headers: { Authorization: 'Omni-JWS aaa.bbb' },
    });
    await expectAuthError(verifyJws(req, env, new ArrayBuffer(0)), 'MalformedEnvelope');
  });
});

describe('verifyJws — header validation', () => {
  it('alg = HS256 → AUTH_UNSUPPORTED_ALG', async () => {
    const body = new Uint8Array();
    const jws = await signJws({
      body,
      header: '{"typ":"JWT","alg":"HS256"}',
    });
    const req = mkReq(jws, { body });
    const ae = await expectAuthError(
      verifyJws(req, env, body.buffer as ArrayBuffer),
      'UnsupportedAlg',
    );
    expect(ae.code).toBe('AUTH_UNSUPPORTED_ALG');
  });
});

describe('verifyJws — signature validation', () => {
  it('mutated signature → AUTH_BAD_SIGNATURE', async () => {
    const body = new Uint8Array();
    const jws = await signJws({ body, mutateSignature: true });
    const req = mkReq(jws, { body });
    const ae = await expectAuthError(
      verifyJws(req, env, body.buffer as ArrayBuffer),
      'BadSignature',
    );
    expect(ae.code).toBe('AUTH_BAD_SIGNATURE');
  });
});

describe('verifyJws — claim-vs-request validation', () => {
  it('ts drift > 300s → AUTH_STALE_TIMESTAMP', async () => {
    const body = new Uint8Array();
    const jws = await signJws({ body, ts: Math.floor(Date.now() / 1000) - 400 });
    const req = mkReq(jws, { body });
    await expectAuthError(verifyJws(req, env, body.buffer as ArrayBuffer), 'StaleTimestamp');
  });

  it('method mismatch → AUTH_MISMATCHED_METHOD_OR_PATH', async () => {
    const body = new Uint8Array();
    const jws = await signJws({ body, method: 'PATCH' });
    const req = mkReq(jws, { method: 'POST', body });
    await expectAuthError(
      verifyJws(req, env, body.buffer as ArrayBuffer),
      'MismatchedMethodOrPath',
    );
  });

  it('path mismatch → AUTH_MISMATCHED_METHOD_OR_PATH', async () => {
    const body = new Uint8Array();
    const jws = await signJws({ body, path: '/v1/report' });
    const req = mkReq(jws, { path: '/v1/upload', body });
    await expectAuthError(
      verifyJws(req, env, body.buffer as ArrayBuffer),
      'MismatchedMethodOrPath',
    );
  });

  it('body_sha256 mismatch → AUTH_BODY_OR_QUERY_MISMATCH', async () => {
    const signedBody = new TextEncoder().encode('signed-body');
    const actualBody = new TextEncoder().encode('different-body');
    const jws = await signJws({ body: signedBody });
    const req = mkReq(jws, { body: actualBody });
    // But signature would fail first if signed_body and actual_body yield
    // different sha256 in claims — we want the body/query check to fire. The
    // test expresses "claim body_sha256 != actual body". Signature is valid
    // over the header+claims; our verifier computes sha256(actual body) and
    // compares to claim, so we get BodyOrQueryMismatch as intended.
    await expectAuthError(
      verifyJws(req, env, actualBody.buffer as ArrayBuffer),
      'BodyOrQueryMismatch',
    );
  });

  it('query_sha256 mismatch → AUTH_BODY_OR_QUERY_MISMATCH', async () => {
    const body = new Uint8Array();
    const jws = await signJws({ body, query: 'a=1', path: '/v1/list', method: 'GET' });
    // Present a different query on the actual request.
    const req = mkReq(jws, { method: 'GET', path: '/v1/list', query: 'a=2' });
    await expectAuthError(verifyJws(req, env, body.buffer as ArrayBuffer), 'BodyOrQueryMismatch');
  });

  it('sanitize_version mismatch → AUTH_UNSUPPORTED_VERSION', async () => {
    const body = new Uint8Array();
    const jws = await signJws({ body, sanitizeVersion: 0 });
    const req = mkReq(jws, { body });
    // env.EXPECTED_SANITIZE_VERSION is unset → default 1; claim 0 != 1.
    await expectAuthError(verifyJws(req, env, body.buffer as ArrayBuffer), 'UnsupportedVersion');
  });
});

describe('verifyJws — denylist', () => {
  it('denylisted pubkey → UNKNOWN_PUBKEY', async () => {
    try {
      await env.STATE.put(`denylist:pubkey:${PUBKEY_HEX}`, '1');
      const body = new Uint8Array();
      const jws = await signJws({ body });
      const req = mkReq(jws, { body });
      const ae = await expectAuthError(
        verifyJws(req, env, body.buffer as ArrayBuffer),
        'UnknownPubkey',
      );
      expect(ae.code).toBe('UNKNOWN_PUBKEY');
    } finally {
      await env.STATE.delete(`denylist:pubkey:${PUBKEY_HEX}`);
    }
  });
});
