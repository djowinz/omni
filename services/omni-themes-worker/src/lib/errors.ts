import type { ErrorBody, ErrorCode, ErrorKind } from "../types";
import type { ContentfulStatusCode } from "hono/utils/http-status";

/**
 * Options for `errorResponse`. `retryAfter` is legacy-compatible with the
 * pre-#008 positional form — callers may still pass a bare number as the 4th
 * argument, but new code should prefer the options object so `kind` / `detail`
 * travel with every error body per worker-api.md §3 "Error categories (D9)".
 */
export interface ErrorResponseOptions {
  kind?: ErrorKind;
  detail?: string;
  retryAfter?: number;
}

/**
 * Build a JSON error `Response` matching the contract envelope in
 * docs/superpowers/specs/contracts/worker-api.md §3. Usable in any context
 * (pure fn, no Hono `Context` needed).
 *
 * The 4th argument is either an options object (new style) or a bare
 * `retry_after` number (legacy, #007 skeleton).
 */
export function errorResponse(
  status: ContentfulStatusCode,
  code: ErrorCode,
  message: string,
  opts?: ErrorResponseOptions | number,
): Response {
  const o: ErrorResponseOptions =
    typeof opts === "number" ? { retryAfter: opts } : (opts ?? {});
  const body: ErrorBody = { error: { code, message } };
  if (o.retryAfter !== undefined) body.error.retry_after = o.retryAfter;
  if (o.kind !== undefined) body.kind = o.kind;
  if (o.detail !== undefined) body.detail = o.detail;
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
}

/** 501 skeleton response; route name is interpolated into the message. */
export function notImplemented(route: string): Response {
  return errorResponse(
    501,
    "NOT_IMPLEMENTED",
    `route ${route} is not implemented yet (sub-spec #007 skeleton)`,
  );
}

/**
 * Single source of truth mapping `(kind, detail?) → (status, code)` per
 * worker-api.md §3 error table and category table. Every request handler that
 * raises a domain error should call this rather than picking codes ad-hoc.
 *
 * The `detail` argument is the D9 sub-kind (`RejectedExecutableMagic`,
 * `SchemaVersionUnsupported`, etc.). When the detail directly corresponds to
 * a contract-listed error code, the matching code is selected; otherwise the
 * kind's default code is used and `detail` travels in the body verbatim.
 *
 * `extras.retryAfter` is threaded through to the response body's
 * `error.retry_after` field only when the mapped code is `RATE_LIMITED`
 * (i.e. `kind === "Quota"` with rate-limit detail). For all other kinds it
 * is ignored — a stray `retry_after` on a non-429 response would violate
 * the worker-api.md §3 envelope.
 */
export function errorFromKind(
  kind: ErrorKind,
  detail: string | undefined,
  message: string,
  extras?: { retryAfter?: number },
): Response {
  const [status, code] = mapKindDetail(kind, detail);
  const retryAfter =
    code === "RATE_LIMITED" ? extras?.retryAfter : undefined;
  return errorResponse(status, code, message, {
    kind,
    detail,
    retryAfter,
  });
}

function mapKindDetail(
  kind: ErrorKind,
  detail: string | undefined,
): [ContentfulStatusCode, ErrorCode] {
  switch (kind) {
    case "Auth":
      return mapAuth(detail);
    case "Quota":
      return mapQuota(detail);
    case "Malformed":
      return mapMalformed(detail);
    case "Unsafe":
      // §3 category table: Unsafe → 422. No pre-retro code row; surface as
      // BAD_REQUEST for grep back-compat with the `code` field while `kind`
      // + `detail` carry the real signal.
      return [422, "BAD_REQUEST"];
    case "Integrity":
      // §3 category table: Integrity → 422. `TOMBSTONED` uses status 410 per
      // legacy row, but kind=Integrity with any other detail maps to 422.
      if (detail === "Tombstoned") return [410, "TOMBSTONED"];
      return [422, "BAD_REQUEST"];
    case "Admin":
      return mapAdmin(detail);
    case "Io":
      return [500, "SERVER_ERROR"];
  }
}

function mapAuth(detail: string | undefined): [ContentfulStatusCode, ErrorCode] {
  switch (detail) {
    case "MalformedEnvelope":
      return [401, "AUTH_MALFORMED_ENVELOPE"];
    case "UnsupportedAlg":
      return [401, "AUTH_UNSUPPORTED_ALG"];
    case "MismatchedMethodOrPath":
      return [401, "AUTH_MISMATCHED_METHOD_OR_PATH"];
    case "BodyOrQueryMismatch":
      return [401, "AUTH_BODY_OR_QUERY_MISMATCH"];
    case "BadSignature":
      return [401, "AUTH_BAD_SIGNATURE"];
    case "StaleTimestamp":
      return [401, "AUTH_STALE_TIMESTAMP"];
    case "UnsupportedVersion":
      // Legacy 426 per §3 table.
      return [426, "AUTH_UNSUPPORTED_VERSION"];
    case "UnknownPubkey":
      return [403, "UNKNOWN_PUBKEY"];
    case "Forbidden":
      return [403, "FORBIDDEN"];
    default:
      return [401, "AUTH_MALFORMED_ENVELOPE"];
  }
}

function mapQuota(detail: string | undefined): [ContentfulStatusCode, ErrorCode] {
  switch (detail) {
    case "TurnstileRequired":
      // Legacy 428 per §3 table.
      return [428, "TURNSTILE_REQUIRED"];
    case "RateLimited":
    default:
      return [429, "RATE_LIMITED"];
  }
}

function mapMalformed(
  detail: string | undefined,
): [ContentfulStatusCode, ErrorCode] {
  switch (detail) {
    case "ManifestInvalid":
      return [400, "MANIFEST_INVALID"];
    case "SizeExceeded":
      // Legacy 413 per §3 table.
      return [413, "SIZE_EXCEEDED"];
    case "NotFound":
      return [404, "NOT_FOUND"];
    case "Conflict":
      return [409, "CONFLICT"];
    case "BadRequest":
    default:
      return [400, "BAD_REQUEST"];
  }
}

function mapAdmin(detail: string | undefined): [ContentfulStatusCode, ErrorCode] {
  switch (detail) {
    case "NotModerator":
      return [403, "ADMIN_NOT_MODERATOR"];
    case "BadTag":
      return [400, "ADMIN_BAD_TAG"];
    case "WouldOrphanArtifacts":
      // Plan #008 W3T12 / spec §9b: conflict with existing resource state → 409.
      return [409, "ADMIN_WOULD_ORPHAN_ARTIFACTS"];
    case "BadValue":
      return [400, "ADMIN_BAD_VALUE"];
    case "NoOp":
      return [400, "ADMIN_NO_OP"];
    default:
      return [403, "ADMIN_NOT_MODERATOR"];
  }
}
