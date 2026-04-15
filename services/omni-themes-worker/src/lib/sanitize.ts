/**
 * Sanitize pipeline entrypoints for the Worker.
 *
 * Two paths exist, both returning an identical shape:
 *
 * - `sanitizeInline`: theme-only fast path. The bundle has already been
 *   verified as a signed bundle by the caller (JWS auth middleware + the
 *   `omni-identity::unpack_signed_bundle` WASM fn). We stream files out of
 *   the handle, pass the map to `omni-sanitize::sanitizeBundle`, and re-pack
 *   as an **unsigned** omni-bundle. The Worker never holds author priv-keys,
 *   so re-signing happens at install time on the consumer host (per invariant
 *   #1: omni-identity is the single signing authority).
 *
 * - `sanitizeViaDO`: routes the raw bytes through the `BUNDLE_PROCESSOR`
 *   Durable Object (DF-keyed for queue fairness; see architectural invariant
 *   #10 — DF, not pubkey, is the durable rate-limit anchor). Bundles with
 *   fonts / images / large payloads go through here because the per-isolate
 *   128 MB ceiling makes inline processing unsafe at the request-handler level.
 *
 * Both return `{ sanitizedBundleBytes, sanitizeReport, canonicalHash }`.
 * The canonical hash is computed from the (post-sanitize) manifest per
 * architectural invariant #6.
 */
import type { Env } from "../env";
import { loadWasm } from "./wasm";

export interface SanitizeReport {
  version: number;
  files: Array<{ path: string; kind: string; actions: string[] }>;
  [key: string]: unknown;
}

export interface SanitizeResult {
  sanitizedBundleBytes: Uint8Array;
  sanitizeReport: SanitizeReport;
  canonicalHash: Uint8Array;
}

/**
 * Inline sanitize path. Caller is responsible for having authenticated the
 * uploader and budget-checked the bundle size. Peak memory for this path is
 * the full uncompressed file set — only call for theme-only bundles within
 * the Worker's comfortable single-request budget.
 */
export async function sanitizeInline(
  bundleBytes: Uint8Array,
  manifest: object,
): Promise<SanitizeResult> {
  const { bundle, identity, sanitize } = await loadWasm();

  // Re-open the signed bundle to iterate files (authoritative vs relying on a
  // caller-passed manifest+files dict). The caller already validated the
  // signature; we pay the reopen cost to guarantee manifest/files consistency
  // at the sanitize boundary.
  const handle = identity.unpackSignedBundle(bundleBytes, undefined);
  const verifiedManifest = handle.manifest();
  const files: Record<string, Uint8Array> = {};
  // Iterate via `nextFile()` — returns `{path, bytes}` or `null` at EOF.
  // eslint-disable-next-line no-constant-condition
  while (true) {
    const entry = handle.nextFile() as { path: string; bytes: Uint8Array } | null;
    if (entry === null) break;
    files[entry.path] = entry.bytes;
  }
  handle.free?.();

  const manifestToUse = manifest ?? verifiedManifest;

  const result = sanitize.sanitizeBundle(manifestToUse, files) as {
    sanitized: Record<string, Uint8Array>;
    report: SanitizeReport;
  };

  // Re-pack as unsigned omni-bundle. The worker never re-signs (invariant #1).
  // `undefined` limits => BundleLimits::DEFAULT, safe for theme-only fast path.
  const sanitizedBundleBytes = bundle.pack(manifestToUse, result.sanitized, undefined);

  const canonicalHash = bundle.canonicalHash(manifestToUse);

  return {
    sanitizedBundleBytes,
    sanitizeReport: result.report,
    canonicalHash,
  };
}

/**
 * Durable-Object-routed sanitize path. The DO lives per-device-fingerprint so
 * concurrent uploads from one device serialize without blocking others. The
 * DO itself performs the same sanitize+repack the inline path does, but with
 * per-isolate memory isolation.
 *
 * Request body is the raw bundle bytes. `dfHex` selects the DO instance.
 */
export async function sanitizeViaDO(
  env: Env,
  bundleBytes: Uint8Array,
  dfHex: string,
): Promise<SanitizeResult> {
  const id = env.BUNDLE_PROCESSOR.idFromName(dfHex);
  const stub = env.BUNDLE_PROCESSOR.get(id);
  const res = await stub.fetch("https://do.internal/sanitize", {
    method: "POST",
    headers: { "content-type": "application/octet-stream" },
    body: bundleBytes,
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`sanitize DO returned ${res.status}: ${text}`);
  }
  // DO returns multipart: sanitized bytes + JSON sidecar. For #007-era the
  // simplest contract is a JSON envelope with base64url-embedded bytes; #009
  // moves this to a binary framing. We decode the base64url here so the
  // caller's contract (returned `SanitizeResult`) is identical to the inline
  // path's return shape.
  const body = (await res.json()) as {
    sanitized_bundle: string; // base64url of bytes
    sanitize_report: SanitizeReport;
    canonical_hash: string; // hex
  };
  return {
    sanitizedBundleBytes: base64UrlDecode(body.sanitized_bundle),
    sanitizeReport: body.sanitize_report,
    canonicalHash: hexDecode(body.canonical_hash),
  };
}

function base64UrlDecode(s: string): Uint8Array {
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/") + "==".slice((s.length + 3) % 4);
  const raw = atob(b64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

function hexDecode(s: string): Uint8Array {
  if (s.length % 2 !== 0) throw new Error("canonical_hash hex: odd length");
  const out = new Uint8Array(s.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(s.slice(i * 2, i * 2 + 2), 16);
  return out;
}
