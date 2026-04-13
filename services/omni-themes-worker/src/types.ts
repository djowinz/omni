import type { Env } from "./env";

/**
 * Structured error code vocabulary from
 * docs/superpowers/specs/contracts/worker-api.md §3.
 * Sub-spec #007 only emits `NOT_IMPLEMENTED`, `NOT_FOUND`, and `SERVER_ERROR`;
 * the full set is declared here so #008 can import without re-defining.
 */
export type ErrorCode =
  | "BAD_SIGNATURE"
  | "STALE_TIMESTAMP"
  | "UNKNOWN_PUBKEY"
  | "RATE_LIMITED"
  | "TURNSTILE_REQUIRED"
  | "BAD_REQUEST"
  | "MANIFEST_INVALID"
  | "SIZE_EXCEEDED"
  | "UNSUPPORTED_VERSION"
  | "NOT_FOUND"
  | "CONFLICT"
  | "FORBIDDEN"
  | "TOMBSTONED"
  | "SERVER_ERROR"
  | "NOT_IMPLEMENTED";

export interface ErrorBody {
  error: {
    code: ErrorCode;
    message: string;
    retry_after?: number;
  };
}

/** Shape of the Hono app used everywhere — `Bindings: Env` gives typed `c.env`. */
export type AppEnv = { Bindings: Env };
