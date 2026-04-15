import type { Env } from "./env";

/**
 * Structured error code vocabulary from
 * docs/superpowers/specs/contracts/worker-api.md §3.
 * Full set exported here so every handler imports from one place.
 */
export type ErrorCode =
  // Auth (§3, JWS envelope)
  | "AUTH_MALFORMED_ENVELOPE"
  | "AUTH_UNSUPPORTED_ALG"
  | "AUTH_MISMATCHED_METHOD_OR_PATH"
  | "AUTH_BODY_OR_QUERY_MISMATCH"
  | "AUTH_BAD_SIGNATURE"
  | "AUTH_STALE_TIMESTAMP"
  | "AUTH_UNSUPPORTED_VERSION"
  | "UNKNOWN_PUBKEY"
  | "FORBIDDEN"
  // Quota
  | "RATE_LIMITED"
  | "TURNSTILE_REQUIRED"
  // Malformed
  | "BAD_REQUEST"
  | "MANIFEST_INVALID"
  | "SIZE_EXCEEDED"
  | "NOT_FOUND"
  | "CONFLICT"
  // Integrity
  | "TOMBSTONED"
  // Admin
  | "ADMIN_NOT_MODERATOR"
  | "ADMIN_BAD_TAG"
  | "ADMIN_WOULD_ORPHAN_ARTIFACTS"
  | "ADMIN_BAD_VALUE"
  | "ADMIN_NO_OP"
  // Io
  | "SERVER_ERROR"
  // Meta
  | "NOT_IMPLEMENTED";

/**
 * Domain categories from retro-005 D9 / worker-api.md §3 "Error categories".
 * Maps to HTTP status via `errorFromKind()` in lib/errors.ts.
 */
export type ErrorKind =
  | "Malformed"
  | "Unsafe"
  | "Integrity"
  | "Io"
  | "Auth"
  | "Quota"
  | "Admin";

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
