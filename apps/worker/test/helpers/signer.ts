/**
 * Shared JWS signer for Tier-B Miniflare integration tests.
 *
 * Mints a compact JWS byte-for-byte equivalent to the shipped `sign_http_jws`
 * oracle in `crates/identity/src/http_jws.rs` — the single source of
 * truth for the HttpJwsClaims wire shape consumed by `src/lib/auth.ts`.
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
 * base64url and NOT hex. The canonical reference implementation lives in
 * `test/auth.test.ts`; this helper carries the same bytes-in/compact-out
 * contract.
 */
import * as ed from "@noble/ed25519";
import { b64urlEncode } from "../../src/lib/base64url";

export interface SignJwsParams {
  method: string;
  path: string;
  /** Body bytes; empty → SHA-256 of the empty string. */
  body?: ArrayBuffer | Uint8Array | string;
  /** Query string (no leading `?`); defaults to `""`. */
  query?: string;
  /** 32-byte Ed25519 seed used to sign the compact JWS. */
  seed: Uint8Array;
  /** 32-byte Ed25519 public key placed in `kid` (std base64). */
  pubkey: Uint8Array;
  /** 32-byte device fingerprint placed in `df` (std base64). */
  df: Uint8Array;
  /** Unix seconds (i64). Defaults to `Math.floor(Date.now() / 1000)`. */
  ts?: number;
  /** Sanitize schema version; defaults to 1. */
  sanitizeVersion?: number;
}

function toBytes(v: ArrayBuffer | Uint8Array | string | undefined): Uint8Array {
  if (v === undefined) return new Uint8Array(0);
  if (typeof v === "string") return new TextEncoder().encode(v);
  if (v instanceof Uint8Array) return v;
  return new Uint8Array(v);
}

function bytesToHex(b: Uint8Array): string {
  let s = "";
  for (let i = 0; i < b.length; i++) s += b[i]!.toString(16).padStart(2, "0");
  return s;
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", bytes);
  return bytesToHex(new Uint8Array(d));
}

/** Standard base64 (RFC 4648 §4, `+/` alphabet, padding preserved). */
function b64StdEncode(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]!);
  return btoa(bin);
}

/**
 * Mint a compact JWS equivalent to the native oracle. Field order in both
 * header and claims JSON is load-bearing — the verifier reconstructs the
 * signing input from these exact bytes.
 */
export async function signJws(p: SignJwsParams): Promise<string> {
  const bodyBytes = toBytes(p.body);
  const queryBytes = new TextEncoder().encode(p.query ?? "");
  const body_sha256 = await sha256Hex(bodyBytes);
  const query_sha256 = await sha256Hex(queryBytes);
  const ts = p.ts ?? Math.floor(Date.now() / 1000);
  const sanitize_version = p.sanitizeVersion ?? 1;

  // Field order matches `HttpJwsClaims` struct declaration in
  // crates/identity/src/http_jws.rs: alg, crv, typ, kid, df, ts,
  // method, path, query_sha256, body_sha256, sanitize_version.
  const claims = {
    alg: "EdDSA",
    crv: "Ed25519",
    typ: "Omni-HTTP-JWS",
    kid: b64StdEncode(p.pubkey),
    df: b64StdEncode(p.df),
    ts,
    method: p.method,
    path: p.path,
    query_sha256,
    body_sha256,
    sanitize_version,
  };

  // jsonwebtoken::Header default serialization: typ first, alg second.
  const headerJson = '{"typ":"Omni-HTTP-JWS","alg":"EdDSA"}';
  const claimsJson = JSON.stringify(claims);

  const headerB64 = b64urlEncode(new TextEncoder().encode(headerJson));
  const payloadB64 = b64urlEncode(new TextEncoder().encode(claimsJson));
  const signingInput = new TextEncoder().encode(`${headerB64}.${payloadB64}`);
  const sig = await ed.signAsync(signingInput, p.seed);
  return `${headerB64}.${payloadB64}.${b64urlEncode(sig)}`;
}
