/**
 * Sanitize pipeline entrypoint for the Worker.
 *
 * `sanitizeViaDO` routes the raw bundle bytes through the `BUNDLE_PROCESSOR`
 * Durable Object (DF-keyed for queue fairness; see architectural invariant
 * #10 — DF, not pubkey, is the durable rate-limit anchor). The DO performs
 * sanitize + manifest-file-hash rewrite + repack on the post-sanitize bytes;
 * this rehash is mandatory because sanitizers mutate file bytes and
 * `bundle.pack` validates `manifest.files[*].sha256`.
 *
 * The Worker never holds an author private key, so the repacked bundle is
 * **unsigned** (per invariant #1: omni-identity is the single signing
 * authority). Downstream consumers verify integrity via `canonical_hash`
 * stored in D1 (`content_hash` column), not via an embedded JWS.
 *
 * An earlier design proposed an inline theme-only fast path; it was removed
 * 2026-04-15 because the post-sanitize rehash must run in every code path.
 *
 * Returns `{ sanitizedBundleBytes, sanitizeReport, canonicalHash }`; the
 * canonical hash is computed from the post-sanitize manifest per invariant #6.
 */
import type { Env } from "../env";

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
  // RFC 4648 §5 — restore '=' padding to a multiple of 4.
  const pad = s.length % 4 === 2 ? "==" : s.length % 4 === 3 ? "=" : "";
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/") + pad;
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
