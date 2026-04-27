import type { Env } from './env';
import { z } from 'zod';

/**
 * Structured error code vocabulary from
 * docs/contracts/worker-api.md §3.
 * Full set exported here so every handler imports from one place.
 */
export type ErrorCode =
  // Auth (§3, JWS envelope)
  | 'AUTH_MALFORMED_ENVELOPE'
  | 'AUTH_UNSUPPORTED_ALG'
  | 'AUTH_MISMATCHED_METHOD_OR_PATH'
  | 'AUTH_BODY_OR_QUERY_MISMATCH'
  | 'AUTH_BAD_SIGNATURE'
  | 'AUTH_STALE_TIMESTAMP'
  | 'AUTH_UNSUPPORTED_VERSION'
  | 'UNKNOWN_PUBKEY'
  | 'FORBIDDEN'
  // Quota
  | 'RATE_LIMITED'
  | 'TURNSTILE_REQUIRED'
  // Malformed
  | 'BAD_REQUEST'
  | 'MANIFEST_INVALID'
  | 'SIZE_EXCEEDED'
  | 'NOT_FOUND'
  | 'CONFLICT'
  // Integrity
  | 'TOMBSTONED'
  // Admin
  | 'ADMIN_NOT_MODERATOR'
  | 'ADMIN_BAD_TAG'
  | 'ADMIN_WOULD_ORPHAN_ARTIFACTS'
  | 'ADMIN_BAD_VALUE'
  | 'ADMIN_NO_OP'
  // Io
  | 'SERVER_ERROR'
  // Meta
  | 'NOT_IMPLEMENTED';

/**
 * Domain categories from retro-005 D9 / worker-api.md §3 "Error categories".
 * Maps to HTTP status via `errorFromKind()` in lib/errors.ts.
 */
export type ErrorKind = 'Malformed' | 'Unsafe' | 'Integrity' | 'Io' | 'Auth' | 'Quota' | 'Admin';

export interface ErrorBody {
  error: {
    code: ErrorCode;
    message: string;
    retry_after?: number;
  };
  kind?: ErrorKind;
  detail?: string;
}

/** Shape of the Hono app used everywhere — `Bindings: Env` gives typed `c.env`. */
export type AppEnv = { Bindings: Env };

/**
 * Wire shape of `GET /v1/author/:pubkey_hex` (and the response envelope
 * mirrored by `PUT /v1/author/me`). Authoritative spec:
 * 2026-04-26-identity-completion-and-display-name §4.2 + §3.4.
 *
 * Fields:
 *   - `pubkey_hex` — 64 lowercase hex chars, the 32-byte Ed25519 author key.
 *   - `fingerprint_hex` — first 6 bytes of SHA-256(pubkey), lowercase hex
 *     (12 chars). Display oracle is `crates/identity/src/fingerprint.rs`.
 *   - `display_name` — user-chosen handle (NFC + trim, 1..=32 chars,
 *     no `\p{Cc}` / `\p{Cs}`). `null` until first `setDisplayName`.
 *   - `joined_at` — Unix seconds; mirrors `authors.created_at` row column.
 *   - `total_uploads` — bumped by every successful `/v1/upload`.
 *
 * Re-exported from `@omni/shared-types` so renderer + host TS consumers bind
 * to the same shape via the Zod schema (per writing-lessons §A8 contract-
 * oracle coverage).
 */
export const AuthorDetailSchema = z.object({
  pubkey_hex: z.string().regex(/^[0-9a-f]{64}$/),
  fingerprint_hex: z.string().regex(/^[0-9a-f]{12}$/),
  display_name: z.string().nullable(),
  joined_at: z.number().int().nonnegative(),
  total_uploads: z.number().int().nonnegative(),
});

export type AuthorDetail = z.infer<typeof AuthorDetailSchema>;
