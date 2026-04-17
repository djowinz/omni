/**
 * Request-signature verification for the Omni Worker.
 *
 * Implements the JWS envelope verification procedure from
 * `docs/contracts/worker-api.md` §2 using the Web Crypto
 * fallback path (see `services/omni-themes-worker/docs/jws-library-decision.md`
 * — the `@tsndr/cloudflare-worker-jwt` library does not support EdDSA so we
 * verify the compact JWS directly with `crypto.subtle.verify('Ed25519', …)`).
 *
 * **Signing-input shape.** Byte-parity oracle is
 * `crates/omni-identity/src/http_jws.rs::sign_http_jws` (delegates to
 * `Keypair::sign_jws`). The oracle signs a standard attached-payload compact
 * JWS:
 *
 *     base64url(header_json) + '.' + base64url(claims_json) + '.' + base64url(sig)
 *
 * where:
 *
 *   header_json = {"typ":"Omni-HTTP-JWS","alg":"EdDSA"}
 *     (jsonwebtoken::Header default serialization: typ, alg)
 *
 *   claims_json = {
 *     "alg":"EdDSA",                    // redundant with header; accepted, not required
 *     "crv":"Ed25519",                  // redundant; accepted, not required
 *     "typ":"Omni-HTTP-JWS",            // redundant; accepted, not required
 *     "kid": "<standard-base64 of 32-byte Ed25519 pubkey>",   // +/ alphabet, padding preserved
 *     "df":  "<standard-base64 of 32-byte device fingerprint>",
 *     "ts":  <i64 unix seconds>,
 *     "method": "<uppercase HTTP method>",
 *     "path": "<request path with leading slash>",
 *     "query_sha256": "<lowercase hex SHA-256 of raw query string, or ''>",
 *     "body_sha256":  "<lowercase hex SHA-256 of raw body, or ''>",
 *     "sanitize_version": <u32>
 *   }
 *
 * The verifier binds to the shipped oracle. `alg` / `crv` / `typ` inside the
 * claims are load-bearing only as documentation — they're not checked here
 * because the signature check already pins the header. `kid` / `df` are
 * standard base64 (`+/` alphabet, padding preserved), NOT base64url and NOT
 * hex — decoded via `atob`.
 *
 * Per architectural invariant #2, the signing key is the author's single
 * Ed25519 identity key (same key used for bundle content signing).
 */
import type { Env } from "../env";
import type { ErrorCode } from "../types";
import { b64urlDecode } from "./base64url";
import { hexEncode } from "./hex";

/** Server's required sanitize-pipeline version. Bumped in lockstep with the
 *  sanitize crate; defaults to 1 when `EXPECTED_SANITIZE_VERSION` is unset. */
const DEFAULT_EXPECTED_SANITIZE_VERSION = 1;

/** Maximum tolerated clock skew between host and Worker, in seconds. Contract §2 step 5. */
export const MAX_TS_DRIFT_SECONDS = 300;

/**
 * Successful verification result handed to route handlers. `pubkey` is the
 * 32-byte Ed25519 author key (from the `kid` claim); `device_fp` is the
 * 32-byte device fingerprint used for quota bookkeeping (invariant #10).
 */
export interface AuthedRequest {
  pubkey: Uint8Array;
  device_fp: Uint8Array;
  ts: number;
  sanitize_version: number;
}

/**
 * Tagged exception thrown by `verifyJws` on any auth failure. A Hono
 * middleware adapter catches this and routes it through
 * `errorFromKind("Auth", detail, message)` so every auth error exits the
 * system through the single mapping table in `src/lib/errors.ts`.
 */
export class AuthError extends Error {
  readonly kind = "Auth" as const;
  readonly detail: string;
  readonly code: ErrorCode;
  constructor(detail: string, code: ErrorCode, message: string) {
    super(message);
    this.name = "AuthError";
    this.detail = detail;
    this.code = code;
  }
}

// ---------------------------------------------------------------------------
// Hashing helpers (encoding helpers now live in src/lib/{hex,base64url}.ts)
// ---------------------------------------------------------------------------

async function sha256Hex(bytes: ArrayBuffer | Uint8Array): Promise<string> {
  const buf = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  const digest = await crypto.subtle.digest("SHA-256", buf);
  return hexEncode(new Uint8Array(digest));
}

/** Constant-time string equality over equal-length lowercase hex strings.
 *  Short-circuit returns leak 1 bit (length match); the threat model in
 *  invariant #0 explicitly allows this for a gamer/hobbyist overlay utility. */
function constantTimeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}

// ---------------------------------------------------------------------------
// Claim shape (payload segment of the compact JWS)
// ---------------------------------------------------------------------------

interface HttpJwsClaims {
  method: string;
  path: string;
  ts: number;
  body_sha256: string;
  query_sha256: string;
  sanitize_version: number;
  kid: string; // standard-base64 of 32-byte Ed25519 pubkey (+/ alphabet, padding preserved)
  df: string; // standard-base64 of 32-byte device fingerprint
}

function asClaims(v: unknown): HttpJwsClaims {
  if (!v || typeof v !== "object") throw new Error("claims not object");
  const o = v as Record<string, unknown>;
  const req = (k: string, t: "string" | "number") => {
    if (typeof o[k] !== t)
      throw new Error(`claims.${k} missing or not ${t}`);
  };
  req("method", "string");
  req("path", "string");
  req("ts", "number");
  req("body_sha256", "string");
  req("query_sha256", "string");
  req("sanitize_version", "number");
  req("kid", "string");
  req("df", "string");
  // `alg`, `crv`, `typ` claim fields are redundant with the JWS header and
  // are accepted-if-present / ignored; not required for verification.
  return o as unknown as HttpJwsClaims;
}

/**
 * Decode a standard-base64 string (RFC 4648 §4, `+/` alphabet, padding
 * required) to bytes via the Web Crypto runtime's `atob`. Separate from the
 * url-safe helpers in `src/lib/base64url.ts` because the shipped host encodes
 * `kid` / `df` with the standard alphabet + padding (see
 * `crates/omni-identity/src/http_jws.rs`). Throws on any non-base64 input.
 */
function b64StdDecode(s: string): Uint8Array {
  // `atob` throws on invalid input; caller wraps in try/catch.
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

// ---------------------------------------------------------------------------
// verifyJws — contract §2 steps 1–10 + denylist check (step 11 of dispatcher)
// ---------------------------------------------------------------------------

/**
 * Verify an `Authorization: Omni-JWS <compact>` header against the buffered
 * request body. Throws `AuthError` on any failure; the Hono middleware adapter
 * routes the error through `errorFromKind("Auth", e.detail, e.message)`.
 *
 * The caller MUST buffer the body before calling (workerd request streams are
 * single-read). Handlers do `const body = await req.arrayBuffer(); const auth
 * = await verifyJws(req, env, body);` then reuse `body` downstream.
 */
export async function verifyJws(
  req: Request,
  env: Env,
  body: ArrayBuffer,
): Promise<AuthedRequest> {
  // Step 1 — parse Authorization header.
  const authHeader = req.headers.get("authorization") ?? req.headers.get("Authorization");
  if (!authHeader) {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      "missing Authorization header",
    );
  }
  const PREFIX = "Omni-JWS ";
  if (!authHeader.startsWith(PREFIX)) {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      "Authorization header must start with 'Omni-JWS '",
    );
  }
  const compact = authHeader.slice(PREFIX.length).trim();

  // Step 2 — split compact JWS.
  const parts = compact.split(".");
  if (parts.length !== 3) {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      "JWS compact form must have 3 segments",
    );
  }
  const [headerB64, payloadB64, sigB64] = parts as [string, string, string];
  if (!headerB64 || !payloadB64 || !sigB64) {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      "JWS compact form has empty segment",
    );
  }

  // Step 3 — decode + validate header. The oracle emits exactly
  // `{"typ":"JWT","alg":"EdDSA"}`; we tolerate extra fields but require alg.
  let headerObj: Record<string, unknown>;
  try {
    headerObj = JSON.parse(new TextDecoder().decode(b64urlDecode(headerB64)));
  } catch {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      "JWS header is not valid JSON",
    );
  }
  if (headerObj["alg"] !== "EdDSA") {
    throw new AuthError(
      "UnsupportedAlg",
      "AUTH_UNSUPPORTED_ALG",
      `JWS alg must be EdDSA, got ${String(headerObj["alg"])}`,
    );
  }

  // Step 4 — decode payload claims.
  let claims: HttpJwsClaims;
  try {
    const raw = JSON.parse(new TextDecoder().decode(b64urlDecode(payloadB64))) as unknown;
    claims = asClaims(raw);
  } catch (e) {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      `JWS claims invalid: ${(e as Error).message}`,
    );
  }

  // Step 5 — reconstruct signing input and verify signature.
  // Signing input = `${headerB64}.${payloadB64}` (UTF-8 bytes). Binds to the
  // oracle in wasm_jws_core.rs.
  const signingInput = new TextEncoder().encode(`${headerB64}.${payloadB64}`);
  const sig = b64urlDecode(sigB64);
  let pubkey: Uint8Array;
  try {
    pubkey = b64StdDecode(claims.kid);
  } catch {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      "claims.kid is not valid base64",
    );
  }
  if (pubkey.length !== 32) {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      `claims.kid must be 32 bytes, got ${pubkey.length}`,
    );
  }

  let verified = false;
  try {
    const key = await crypto.subtle.importKey(
      "raw",
      pubkey as BufferSource,
      { name: "Ed25519" },
      false,
      ["verify"],
    );
    verified = await crypto.subtle.verify(
      "Ed25519",
      key,
      sig as BufferSource,
      signingInput as BufferSource,
    );
  } catch (e) {
    // Malformed signature bytes or key length surface as BadSignature — the
    // envelope-level parsing already ran in steps 1–4.
    throw new AuthError(
      "BadSignature",
      "AUTH_BAD_SIGNATURE",
      `JWS signature verify threw: ${(e as Error).message}`,
    );
  }
  if (!verified) {
    throw new AuthError(
      "BadSignature",
      "AUTH_BAD_SIGNATURE",
      "JWS signature did not verify",
    );
  }

  // Step 6 — timestamp drift. Skew in either direction is a failure; the
  // contract names it `StaleTimestamp` regardless of sign.
  const now = Math.floor(Date.now() / 1000);
  if (!Number.isFinite(claims.ts) || Math.abs(now - claims.ts) > MAX_TS_DRIFT_SECONDS) {
    throw new AuthError(
      "StaleTimestamp",
      "AUTH_STALE_TIMESTAMP",
      `ts drift > ${MAX_TS_DRIFT_SECONDS}s (now=${now}, claim=${claims.ts})`,
    );
  }

  // Step 7 — method / path mismatch.
  const reqUrl = new URL(req.url);
  if (claims.method !== req.method) {
    throw new AuthError(
      "MismatchedMethodOrPath",
      "AUTH_MISMATCHED_METHOD_OR_PATH",
      `method mismatch: claim=${claims.method} actual=${req.method}`,
    );
  }
  if (claims.path !== reqUrl.pathname) {
    throw new AuthError(
      "MismatchedMethodOrPath",
      "AUTH_MISMATCHED_METHOD_OR_PATH",
      `path mismatch: claim=${claims.path} actual=${reqUrl.pathname}`,
    );
  }

  // Step 8 — body_sha256 + query_sha256 match.
  const actualBodyHash = await sha256Hex(body);
  if (!constantTimeEqual(actualBodyHash, claims.body_sha256.toLowerCase())) {
    throw new AuthError(
      "BodyOrQueryMismatch",
      "AUTH_BODY_OR_QUERY_MISMATCH",
      "body_sha256 claim does not match request body",
    );
  }
  const queryString = reqUrl.search.startsWith("?") ? reqUrl.search.slice(1) : reqUrl.search;
  const actualQueryHash = await sha256Hex(new TextEncoder().encode(queryString));
  if (!constantTimeEqual(actualQueryHash, claims.query_sha256.toLowerCase())) {
    throw new AuthError(
      "BodyOrQueryMismatch",
      "AUTH_BODY_OR_QUERY_MISMATCH",
      "query_sha256 claim does not match request query string",
    );
  }

  // Step 9 — sanitize_version gate. Server's expected version lives in env
  // (bumped in lockstep with the sanitize crate); default to 1.
  const expectedSanitize =
    parseSanitizeVersion((env as Env & { EXPECTED_SANITIZE_VERSION?: string })
      .EXPECTED_SANITIZE_VERSION) ?? DEFAULT_EXPECTED_SANITIZE_VERSION;
  if (claims.sanitize_version !== expectedSanitize) {
    throw new AuthError(
      "UnsupportedVersion",
      "AUTH_UNSUPPORTED_VERSION",
      `sanitize_version mismatch: claim=${claims.sanitize_version} expected=${expectedSanitize}`,
    );
  }

  // Step 10 — denylist. Per invariant #10, device-fingerprint denylist is a
  // separate layer handled by the rate-limit middleware; this check is the
  // pubkey anchor specifically (`UNKNOWN_PUBKEY` / `denylist:pubkey:<hex>`).
  // KV key uses hex of the 32 verified pubkey bytes — stable across encoding
  // drift in the `kid` claim (which is standard base64 on the wire).
  const pubkeyHex = hexEncode(pubkey);
  const deniedPub = await env.STATE.get(`denylist:pubkey:${pubkeyHex}`);
  if (deniedPub !== null) {
    throw new AuthError(
      "UnknownPubkey",
      "UNKNOWN_PUBKEY",
      "pubkey is denylisted",
    );
  }

  // All gates passed — decode df and hand the auth context to the caller.
  let device_fp: Uint8Array;
  try {
    device_fp = b64StdDecode(claims.df);
  } catch {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      "claims.df is not valid base64",
    );
  }
  if (device_fp.length !== 32) {
    throw new AuthError(
      "MalformedEnvelope",
      "AUTH_MALFORMED_ENVELOPE",
      `claims.df must be 32 bytes, got ${device_fp.length}`,
    );
  }

  return {
    pubkey,
    device_fp,
    ts: claims.ts,
    sanitize_version: claims.sanitize_version,
  };
}

function parseSanitizeVersion(v: string | undefined): number | null {
  if (v === undefined || v === null || v === "") return null;
  const n = parseInt(v, 10);
  return Number.isFinite(n) ? n : null;
}
