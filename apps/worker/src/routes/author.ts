import { Hono } from 'hono';
import type { AppEnv } from '../types';
import { errorResponse, errorFromKind } from '../lib/errors';
import { AuthError, verifyJws } from '../lib/auth';
import { hexEncode, hexDecode } from '../lib/hex';
import { validateDisplayName } from '../lib/display_name';

/**
 * `GET /v1/author/:pubkey_hex` (public, no JWS) and
 * `PUT /v1/author/me` (JWS-authenticated).
 *
 * Authoritative spec: 2026-04-26-identity-completion-and-display-name §4.2
 * (route shapes) + §3.4 (display_name validation rules — NFC normalize, trim,
 * 1..=32 chars, no `\p{Cc}` or `\p{Cs}`, no banlist v1).
 *
 * GET returns AuthorDetail `{ pubkey_hex, fingerprint_hex, display_name,
 * joined_at, total_uploads }` or 404 with the standard error envelope.
 * `fingerprint_hex` is the first 6 bytes of SHA-256(pubkey) rendered lowercase
 * hex (12 chars), matching the on-disk `Fingerprint` shipped by
 * `crates/identity/src/fingerprint.rs`.
 *
 * PUT verifies an `Authorization: Omni-JWS <compact>` envelope via the same
 * `verifyJws` helper used by /v1/report + /v1/upload, then upserts
 * `authors.display_name`. The pubkey is taken from the JWS `kid` claim — the
 * route name "/me" reflects "the author identified by your JWS." Validation
 * mirrors §3.4 EXACTLY so host-side `identity.setDisplayName` and worker-side
 * upload `display_name` form field agree byte-for-byte. The shared
 * `validateDisplayName` helper at `apps/worker/src/lib/display_name.ts` is
 * the single source of those rules per plan §T5 step 5.3.
 */

const app = new Hono<AppEnv>();

/**
 * Hex-validate a string in-place. The Worker's `hex.ts` exports
 * `hexDecode`-which-throws but no boolean validator; cheaper to inline a
 * char-class check than to wrap a try/catch around the decoder.
 */
function isValid64Hex(s: string): boolean {
  if (s.length !== 64) return false;
  for (let i = 0; i < s.length; i++) {
    const c = s.charCodeAt(i);
    const isLowerHex = (c >= 0x30 && c <= 0x39) || (c >= 0x61 && c <= 0x66);
    if (!isLowerHex) return false;
  }
  return true;
}

// ---------------------------------------------------------------------------
// GET /v1/author/:pubkey_hex — public, no JWS.
// ---------------------------------------------------------------------------

app.get('/:pubkey_hex', async (c) => {
  const pubkeyHex = c.req.param('pubkey_hex').toLowerCase();
  if (!isValid64Hex(pubkeyHex)) {
    return errorFromKind('Malformed', 'BadRequest', 'pubkey_hex must be 64 lowercase hex chars');
  }

  const pubkeyBytes = hexDecode(pubkeyHex);

  const row = await c.env.META.prepare(
    'SELECT display_name, created_at, total_uploads FROM authors WHERE pubkey = ?',
  )
    .bind(pubkeyBytes)
    .first<{ display_name: string | null; created_at: number; total_uploads: number }>();

  if (!row) {
    return errorResponse(404, 'NOT_FOUND', 'no such author', {
      kind: 'Malformed',
      detail: 'NotFound',
    });
  }

  // fingerprint_hex = first 6 bytes of SHA-256(pubkey), rendered lowercase
  // hex (12 chars). Matches the Rust `Fingerprint::to_hex` shape from
  // crates/identity/src/fingerprint.rs.
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', pubkeyBytes));
  const fingerprintHex = hexEncode(digest.subarray(0, 6));

  return c.json({
    pubkey_hex: pubkeyHex,
    fingerprint_hex: fingerprintHex,
    display_name: row.display_name,
    joined_at: row.created_at,
    total_uploads: row.total_uploads,
  });
});

// ---------------------------------------------------------------------------
// PUT /v1/author/me — JWS-authenticated.
// ---------------------------------------------------------------------------

app.put('/me', async (c) => {
  // Buffer the body once — workerd request streams are single-read and
  // verifyJws needs the bytes for the body_sha256 claim check.
  const bodyBuf = await c.req.arrayBuffer();

  // Step 1 — JWS auth. AuthError carries the structured detail; route it
  // through the single mapping table in `errors.ts`.
  let auth;
  try {
    auth = await verifyJws(c.req.raw, c.env, bodyBuf);
  } catch (e) {
    if (e instanceof AuthError) {
      return errorFromKind('Auth', e.detail, e.message);
    }
    throw e;
  }

  // Step 2 — parse + validate body shape.
  let parsed: unknown;
  try {
    parsed = JSON.parse(new TextDecoder().decode(bodyBuf));
  } catch {
    return errorFromKind('Malformed', 'BadRequest', 'body is not valid JSON');
  }
  if (!parsed || typeof parsed !== 'object') {
    return errorFromKind('Malformed', 'BadRequest', 'body must be a JSON object');
  }

  // Step 3 — §3.4 display_name validation.
  const v = validateDisplayName((parsed as { display_name?: unknown }).display_name);
  if ('err' in v) {
    return errorFromKind('Malformed', 'BadRequest', v.err);
  }

  // Step 4 — upsert. ON CONFLICT preserves total_uploads / is_new_creator /
  // is_denied / created_at and only overwrites display_name. Per §4.2:
  //
  //   INSERT INTO authors (pubkey, display_name, created_at,
  //                        total_uploads, is_new_creator, is_denied)
  //   VALUES (?, ?, ?, 0, 1, 0)
  //   ON CONFLICT(pubkey) DO UPDATE SET display_name = excluded.display_name;
  const nowSec = Math.floor(Date.now() / 1000);
  await c.env.META.prepare(
    `INSERT INTO authors (pubkey, display_name, created_at, total_uploads, is_new_creator, is_denied)
     VALUES (?, ?, ?, 0, 1, 0)
     ON CONFLICT(pubkey) DO UPDATE SET display_name = excluded.display_name`,
  )
    .bind(auth.pubkey, v.ok, nowSec)
    .run();

  return c.json({
    pubkey_hex: hexEncode(auth.pubkey),
    display_name: v.ok,
  });
});

export default app;
