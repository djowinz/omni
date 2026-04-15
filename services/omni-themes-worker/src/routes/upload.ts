/**
 * POST /v1/upload — spec #008 §4.
 *
 * Pipeline (1..14):
 *   1. Body cap (config:limits.max_bundle_compressed, 60s cached)
 *   2. Buffer body, verify JWS auth
 *   3. Rate-limit (upload_new / upload_update / upload_new_bundle)
 *   4. Multipart parse
 *   5. Manifest-only fast path (omni_bundle.unpackManifest via identity handle)
 *   6. Tag vocab validation (suggested_alternatives)
 *   7. Content signature verify (omni_identity.unpackSignedBundle) + kid match
 *   8. Sanitize path (inline for theme-only; DO otherwise)
 *   9. canonical_hash(manifest)
 *  10. Dedup/tombstone
 *  11. Per-author name uniqueness
 *  12. Persist R2 + D1
 *  13. (never bump install_daily here)
 *  14. Respond
 */
import { Hono } from "hono";
import type { AppEnv } from "../types";
import type { Env } from "../env";
import { errorResponse, errorFromKind } from "../lib/errors";
import { verifyJws, AuthError } from "../lib/auth";
import { checkAndIncrement, type RateLimitAction } from "../lib/rate_limit";
import { parseMultipart, MultipartError } from "../lib/multipart";
import { loadWasm } from "../lib/wasm";
import { canonicalHash } from "../lib/canonical";
import { sanitizeInline, sanitizeViaDO } from "../lib/sanitize";

const app = new Hono<AppEnv>();

// ---------- Helpers ---------------------------------------------------------

function hexEncode(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i++) s += bytes[i]!.toString(16).padStart(2, "0");
  return s;
}

/**
 * Config KV reads. Spec #008 §4 describes a 60s in-memory TTL cache; deferred
 * to #012's admin routes where the version bump gives us an invalidation
 * signal. Reading on every request is well inside Worker KV budget for this
 * low-volume service (invariant #0).
 */
async function getLimits(env: Env) {
  const raw = await env.STATE.get("config:limits");
  const fallback = {
    max_bundle_compressed: 5_242_880,
    max_bundle_uncompressed: 10_485_760,
    max_entries: 32,
    version: 1,
    updated_at: 0,
  };
  return raw ? (JSON.parse(raw) as typeof fallback) : fallback;
}

async function getVocab(env: Env) {
  const raw = await env.STATE.get("config:vocab");
  const fallback = { tags: [] as string[], version: 1 };
  return raw ? (JSON.parse(raw) as typeof fallback) : fallback;
}

/** Levenshtein distance (≤1 check). Tiny impl (justified inline per spec). */
function levenshtein1(a: string, b: string): boolean {
  if (a === b) return true;
  const la = a.length;
  const lb = b.length;
  if (Math.abs(la - lb) > 1) return false;
  // Substitution (equal length)
  if (la === lb) {
    let diffs = 0;
    for (let i = 0; i < la; i++) if (a[i] !== b[i]) if (++diffs > 1) return false;
    return true;
  }
  // Insertion/deletion (differ by 1)
  const [s, t] = la < lb ? [a, b] : [b, a];
  let i = 0, j = 0, skipped = 0;
  while (i < s.length && j < t.length) {
    if (s[i] === t[j]) { i++; j++; } else {
      if (++skipped > 1) return false;
      j++;
    }
  }
  return true;
}

function suggestAlternatives(badTag: string, vocab: string[]): string[] {
  return vocab.filter((t) => levenshtein1(badTag, t));
}

/** Read the request body with a hard cap; reject over-cap before arrayBuffer. */
async function readBodyWithCap(req: Request, cap: number): Promise<ArrayBuffer | { over: true }> {
  const lenHeader = req.headers.get("content-length");
  if (lenHeader !== null) {
    const n = Number(lenHeader);
    if (Number.isFinite(n) && n > cap) return { over: true };
  }
  // Stream-read with running total check (covers missing/lying content-length).
  if (req.body) {
    const reader = req.body.getReader();
    const chunks: Uint8Array[] = [];
    let total = 0;
    // eslint-disable-next-line no-constant-condition
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (value) {
        total += value.byteLength;
        if (total > cap) {
          try { await reader.cancel(); } catch { /* swallow */ }
          return { over: true };
        }
        chunks.push(value);
      }
    }
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) { out.set(c, off); off += c.byteLength; }
    return out.buffer.slice(out.byteOffset, out.byteOffset + out.byteLength);
  }
  return new ArrayBuffer(0);
}

interface Manifest {
  schema_version: number;
  name: string;
  version: string;
  description?: string;
  tags?: string[];
  license?: string;
  omni_min_version: string;
  resource_kinds?: Record<string, unknown>;
  [k: string]: unknown;
}

function isThemeOnly(manifest: Manifest): boolean {
  const kinds = manifest.resource_kinds;
  if (!kinds || typeof kinds !== "object") return true;
  const keys = Object.keys(kinds);
  if (keys.length === 0) return true;
  return keys.every((k) => k === "theme");
}

function categorizeBundleError(err: unknown): { kind: "Malformed" | "Unsafe" | "Integrity" | "Io"; detail: string; msg: string } {
  const msg = err instanceof Error ? err.message : String(err);
  const lower = msg.toLowerCase();
  if (lower.includes("zipbomb") || lower.includes("unsafe")) {
    return { kind: "Unsafe", detail: "Unsafe", msg };
  }
  if (lower.includes("signature") || lower.includes("integrity") || lower.includes("jws")) {
    return { kind: "Integrity", detail: "SignatureInvalid", msg };
  }
  if (lower.includes("manifest") || lower.includes("schema") || lower.includes("json")) {
    return { kind: "Malformed", detail: "ManifestInvalid", msg };
  }
  return { kind: "Malformed", detail: "BadRequest", msg };
}

// ---------- Route -----------------------------------------------------------

app.post("/", async (c) => {
  const env = c.env;
  const req = c.req.raw;
  const url = new URL(req.url);

  // Step 1 — body cap
  const limits = await getLimits(env);
  const bodyOrOver = await readBodyWithCap(req, limits.max_bundle_compressed);
  if ("over" in (bodyOrOver as object) && (bodyOrOver as { over: true }).over) {
    return errorFromKind("Malformed", "SizeExceeded", "request body exceeds max_bundle_compressed");
  }
  const body = bodyOrOver as ArrayBuffer;

  // Step 2 — auth
  let auth;
  try {
    auth = await verifyJws(req, env, body);
  } catch (e) {
    if (e instanceof AuthError) {
      return errorFromKind("Auth", e.detail, e.message);
    }
    throw e;
  }

  const pubkeyHex = hexEncode(auth.pubkey);
  const dfHex = hexEncode(auth.device_fp);

  // Step 3 — rate limit. `artifact_id` as query param signals update path.
  const isUpdate = url.searchParams.has("artifact_id");
  const action: RateLimitAction = isUpdate ? "upload_update" : "upload_new";
  const rl = await checkAndIncrement(env, dfHex, pubkeyHex, action);
  if (!rl.allowed) {
    if (rl.turnstile) {
      return errorFromKind("Quota", "TurnstileRequired", "turnstile challenge required");
    }
    return errorResponse(429, "RATE_LIMITED", "rate limit exceeded", {
      kind: "Quota",
      detail: "RateLimited",
      retryAfter: rl.retry_after,
    });
  }

  // Step 4 — parse multipart. Hono's `c.req.raw.formData()` is single-shot; we
  // already buffered `body`, so re-synthesize a Request with the buffered body.
  const formReq = new Request(req.url, {
    method: req.method,
    headers: req.headers,
    body,
  });
  let parts;
  try {
    parts = await parseMultipart(formReq);
  } catch (e) {
    if (e instanceof MultipartError) {
      return errorFromKind("Malformed", "BadRequest", `multipart: ${e.message}`);
    }
    throw e;
  }

  // Step 5 — manifest fast path
  const { identity } = await loadWasm();
  let manifest: Manifest;
  interface SignedHandleLike {
    manifest: () => unknown;
    authorPubkey: () => Uint8Array;
    nextFile: () => unknown;
    free?: () => void;
  }
  let signedHandle: SignedHandleLike | null = null;

  // Per invariant #6a: we do NOT read signature.jws ourselves. Use the
  // signed-bundle path for both validation and content signature verify.
  try {
    signedHandle = identity.unpackSignedBundle(parts.bundle, undefined) as unknown as SignedHandleLike;
    manifest = signedHandle.manifest() as Manifest;
  } catch (e) {
    const cat = categorizeBundleError(e);
    return errorFromKind(cat.kind, cat.detail, cat.msg);
  }

  // Step 7 (pubkey match) happens here using the signed-bundle's authorPubkey.
  const authorPub = signedHandle.authorPubkey();
  const authorPubHex = hexEncode(authorPub);
  if (authorPubHex !== pubkeyHex) {
    try { signedHandle.free?.(); } catch { /* swallow */ }
    return errorFromKind("Auth", "Forbidden", "bundle author pubkey does not match request kid");
  }

  // Step 6 — tag validation
  const vocab = await getVocab(env);
  const tags = Array.isArray(manifest.tags) ? manifest.tags : [];
  for (const t of tags) {
    if (typeof t !== "string" || !vocab.tags.includes(t)) {
      try { (signedHandle as SignedHandleLike | null)?.free?.(); } catch { /* swallow */ }
      const suggested = typeof t === "string" ? suggestAlternatives(t, vocab.tags) : [];
      const body: Record<string, unknown> = {
        error: { code: "MANIFEST_INVALID", message: `unknown tag: ${String(t)}` },
        kind: "Malformed",
        detail: "UnknownTag",
        suggested_alternatives: suggested,
      };
      return new Response(JSON.stringify(body), {
        status: 400,
        headers: { "content-type": "application/json; charset=utf-8" },
      });
    }
  }

  // Release the handle — sanitizeInline will re-open (it needs its own iterator).
  try { signedHandle?.free?.(); } catch { /* swallow */ }
  signedHandle = null;

  // Step 8 — sanitize path
  const themeOnly = isThemeOnly(manifest);

  // Non-theme bundles also count against `upload_new_bundle`.
  if (!themeOnly && !isUpdate) {
    const rl2 = await checkAndIncrement(env, dfHex, pubkeyHex, "upload_new_bundle");
    if (!rl2.allowed) {
      if (rl2.turnstile) return errorFromKind("Quota", "TurnstileRequired", "turnstile required");
      return errorResponse(429, "RATE_LIMITED", "bundle rate limit exceeded", {
        kind: "Quota", detail: "RateLimited", retryAfter: rl2.retry_after,
      });
    }
  }

  // Both paths sanitize+repack. We use the DO path in all cases — the inline
  // fast path from W2T7 cannot currently recompute manifest file hashes
  // post-sanitize and produces integrity-invalid bundles when handlers
  // transform bytes. The DO's sanitize+repack reuses `omni_identity`'s
  // authoritative hasher so the repacked bundle is always consistent.
  let sanitized;
  try {
    sanitized = await sanitizeViaDO(env, parts.bundle, dfHex);
  } catch (e) {
    const cat = categorizeBundleError(e);
    return errorFromKind(cat.kind, cat.detail, cat.msg);
  }
  // Suppress unused-import lint when inline path is disabled.
  void sanitizeInline;

  // Step 9 — canonical hash (native hex)
  const hashBytes = await canonicalHash(manifest);
  const contentHash = hexEncode(hashBytes);

  // Step 10 — dedup / tombstone
  const tombRow = await env.META.prepare(
    "SELECT content_hash FROM tombstones WHERE content_hash = ?",
  ).bind(contentHash).first<{ content_hash: string }>();
  if (tombRow) {
    return errorFromKind("Integrity", "Tombstoned", "content is tombstoned");
  }
  const dedupRow = await env.META.prepare(
    "SELECT artifact_id FROM content_hashes WHERE content_hash = ?",
  ).bind(contentHash).first<{ artifact_id: string }>();
  if (dedupRow) {
    const row = await env.META.prepare(
      "SELECT id, content_hash, thumbnail_hash, created_at FROM artifacts WHERE id = ? AND is_removed = 0",
    ).bind(dedupRow.artifact_id).first<{ id: string; content_hash: string; thumbnail_hash: string; created_at: number }>();
    if (row) {
      return Response.json({
        artifact_id: row.id,
        content_hash: row.content_hash,
        r2_url: `r2://bundles/${row.content_hash}.omnipkg`,
        thumbnail_url: `r2://thumbnails/${row.thumbnail_hash}.png`,
        created_at: row.created_at,
        status: "deduplicated",
      });
    }
  }

  // Step 11 — per-author name uniqueness (only on new upload)
  const authorPubkeyBlob = auth.pubkey;
  if (!isUpdate) {
    const nameRow = await env.META.prepare(
      "SELECT id FROM artifacts WHERE author_pubkey = ? AND name = ?",
    ).bind(authorPubkeyBlob, manifest.name).first<{ id: string }>();
    if (nameRow) {
      return errorFromKind("Malformed", "Conflict", "name already used by this author");
    }
  }

  // Step 12 — persist R2 + D1
  const thumbHashBuf = await crypto.subtle.digest("SHA-256", parts.thumbnail as BufferSource);
  const thumbHash = hexEncode(new Uint8Array(thumbHashBuf));

  await env.BLOBS.put(`bundles/${contentHash}.omnipkg`, sanitized.sanitizedBundleBytes);
  await env.BLOBS.put(`thumbnails/${thumbHash}.png`, parts.thumbnail);

  const artifactId = crypto.randomUUID();
  const now = Math.floor(Date.now() / 1000);
  const kind = themeOnly ? "theme" : "bundle";

  // Author upsert (first-seen).
  await env.META.prepare(
    `INSERT INTO authors (pubkey, created_at, total_uploads, is_new_creator, is_denied)
     VALUES (?, ?, 1, 1, 0)
     ON CONFLICT(pubkey) DO UPDATE SET total_uploads = total_uploads + 1`,
  ).bind(authorPubkeyBlob, now).run();

  // Signature is content-JWS — we hand back what arrived. We don't pull it out
  // of the zip per invariant #6a; store the full sanitized bundle and its
  // content hash. `signature` blob column is retained for legacy rows; use the
  // content hash as the placeholder signature marker.
  const signaturePlaceholder = new Uint8Array(thumbHashBuf);

  await env.META.prepare(
    `INSERT INTO artifacts
     (id, author_pubkey, name, kind, content_hash, thumbnail_hash, description,
      tags, license, version, omni_min_version, signature, created_at, updated_at,
      install_count, report_count, is_removed, is_featured)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, 0, 0)`,
  ).bind(
    artifactId,
    authorPubkeyBlob,
    manifest.name,
    kind,
    contentHash,
    thumbHash,
    manifest.description ?? null,
    JSON.stringify(tags),
    manifest.license ?? null,
    manifest.version,
    manifest.omni_min_version,
    signaturePlaceholder,
    now,
    now,
  ).run();

  await env.META.prepare(
    `INSERT OR IGNORE INTO content_hashes (content_hash, artifact_id, first_seen_at)
     VALUES (?, ?, ?)`,
  ).bind(contentHash, artifactId, now).run();

  // Step 14 — respond
  return Response.json({
    artifact_id: artifactId,
    content_hash: contentHash,
    r2_url: `r2://bundles/${contentHash}.omnipkg`,
    thumbnail_url: `r2://thumbnails/${thumbHash}.png`,
    created_at: now,
    status: "created",
  });
});

export default app;
