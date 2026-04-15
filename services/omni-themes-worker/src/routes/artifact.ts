/**
 * Artifact CRUD — spec #008 §8 (GET), §9 (PATCH), §10 (DELETE).
 * Contract worker-api.md §4.4 / §4.5 / §4.6.
 *
 * GET is unauthenticated but opportunistically verifies an `Authorization:
 * Omni-JWS` header so that moderators can see the `reports` count that is
 * redacted from public responses (§8).
 *
 * PATCH / DELETE are author-only. Authored pubkey MUST equal the JWS `kid`.
 *
 * NOTE on PATCH pipeline reuse: W3T10 (upload) is being written concurrently
 * in Wave C. At the time of this change, `processUploadBody` did NOT exist
 * as a shared helper, so this file implements the owner + hash-compare half
 * of the PATCH flow natively and defers the full sanitize/repack pipeline to
 * the `BundleProcessor` DO — the same DO the upload route invokes. W4 cleanup
 * should factor the `multipart → DO → persist` code path into a shared helper
 * in `src/lib/upload_pipeline.ts` once both routes are in-tree. See report.
 */
import { Hono } from "hono";
import type { AppEnv } from "../types";
import { errorResponse, errorFromKind } from "../lib/errors";
import { verifyJws, AuthError } from "../lib/auth";
import { isModerator } from "../lib/moderator";
import { checkAndIncrement } from "../lib/rate_limit";

const app = new Hono<AppEnv>();

interface ArtifactFullRow {
  id: string;
  author_pubkey: ArrayBuffer;
  name: string;
  kind: string;
  content_hash: string;
  thumbnail_hash: string;
  description: string | null;
  tags: string | null;
  license: string | null;
  version: string;
  omni_min_version: string;
  signature: ArrayBuffer;
  created_at: number;
  updated_at: number;
  install_count: number;
  report_count: number;
  is_removed: number;
  is_featured: number;
}

function hexEncode(bytes: Uint8Array): string {
  let s = "";
  for (let i = 0; i < bytes.length; i++) s += bytes[i]!.toString(16).padStart(2, "0");
  return s;
}

function bytesEqual(a: Uint8Array, b: Uint8Array): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i]! ^ b[i]!;
  return diff === 0;
}

function parseTags(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const p = JSON.parse(raw);
    if (Array.isArray(p)) return p.filter((t): t is string => typeof t === "string");
  } catch {
    /* fall through */
  }
  return [];
}

function deriveStatus(row: ArtifactFullRow): "live" | "tombstoned" | "moderation_hold" {
  if (row.is_removed) return "tombstoned";
  // `moderation_hold` has no dedicated column in the #007 schema; future
  // moderation work will add one. Until then, a live artifact with any
  // report_count is still public (reports are evidence, not holds).
  return "live";
}

async function loadArtifact(
  env: AppEnv["Bindings"],
  id: string,
): Promise<ArtifactFullRow | null> {
  return env.META.prepare(
    `SELECT id, author_pubkey, name, kind, content_hash, thumbnail_hash,
            description, tags, license, version, omni_min_version, signature,
            created_at, updated_at, install_count, report_count, is_removed,
            is_featured
     FROM artifacts WHERE id = ?`,
  )
    .bind(id)
    .first<ArtifactFullRow>();
}

function artifactResponse(row: ArtifactFullRow, includeReports: boolean): unknown {
  const pubHex = hexEncode(new Uint8Array(row.author_pubkey));
  const body: Record<string, unknown> = {
    artifact_id: row.id,
    kind: row.kind,
    name: row.name,
    description: row.description ?? "",
    tags: parseTags(row.tags),
    license: row.license ?? "",
    version: row.version,
    omni_min_version: row.omni_min_version,
    manifest: null, // filled on a best-effort basis from R2 in a later pass;
    // §8 lists `manifest` as a field but building it requires cracking the
    // bundle open. The download route exposes the manifest via
    // X-Omni-Manifest; the artifact GET here stays metadata-only until W4T14
    // wires the shared manifest-extraction helper.
    content_hash: row.content_hash,
    r2_url: `/v1/download/${row.id}`,
    thumbnail_url: `/v1/thumbnail/${row.thumbnail_hash}`,
    author_pubkey: pubHex,
    author_fingerprint_hex: pubHex.slice(0, 12),
    installs: row.install_count,
    created_at: row.created_at,
    updated_at: row.updated_at,
    status: deriveStatus(row),
  };
  if (includeReports) body.reports = row.report_count;
  return body;
}

// ---------------------------------------------------------------------------
// GET /v1/artifact/:id  (unauthenticated; moderator optional for reports)
// ---------------------------------------------------------------------------

app.get("/:id", async (c) => {
  const env = c.env;
  const id = c.req.param("id");
  const row = await loadArtifact(env, id);
  if (!row) return errorFromKind("Malformed", "NotFound", "artifact not found");

  // Opportunistic auth — if the caller presented a JWS header, verify it so
  // moderators can see the `reports` count. Failure is non-fatal: we just
  // serve the public view. (Contract: route is unauthenticated by default.)
  let includeReports = false;
  const authHeader =
    c.req.raw.headers.get("authorization") ?? c.req.raw.headers.get("Authorization");
  if (authHeader) {
    try {
      const authed = await verifyJws(c.req.raw, env, new ArrayBuffer(0));
      const pubHex = hexEncode(authed.pubkey);
      if (isModerator(pubHex, env)) includeReports = true;
    } catch {
      // Bad signatures on a public GET are ignored — we just don't expose
      // the moderator-only field. This avoids turning a transient auth error
      // into a hard 401 on a route that doesn't require auth.
    }
  }

  return new Response(JSON.stringify(artifactResponse(row, includeReports)), {
    status: 200,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
});

// ---------------------------------------------------------------------------
// PATCH /v1/artifact/:id  (author-only; dispatches to BundleProcessor DO)
// ---------------------------------------------------------------------------

app.patch("/:id", async (c) => {
  const env = c.env;
  const id = c.req.param("id");
  const body = await c.req.raw.arrayBuffer();

  // 1. Auth.
  let authed;
  try {
    authed = await verifyJws(c.req.raw, env, body);
  } catch (e) {
    if (e instanceof AuthError) return errorFromKind("Auth", e.detail, e.message);
    throw e;
  }

  // 2. Load existing artifact.
  const row = await loadArtifact(env, id);
  if (!row) return errorFromKind("Malformed", "NotFound", "artifact not found");

  // 3. Author match (bytewise on the raw 32-byte pubkey).
  const rowPub = new Uint8Array(row.author_pubkey);
  if (!bytesEqual(rowPub, authed.pubkey)) {
    return errorFromKind("Auth", "Forbidden", "not the author of this artifact");
  }

  // 4. Rate limit.
  const dfHex = hexEncode(authed.device_fp);
  const pubHex = hexEncode(authed.pubkey);
  const rl = await checkAndIncrement(env, dfHex, pubHex, "upload_update");
  if (!rl.allowed) {
    if (rl.turnstile) return errorFromKind("Quota", "TurnstileRequired", "turnstile required");
    return errorResponse(429, "RATE_LIMITED", "update rate limit exceeded", {
      kind: "Quota", detail: "RateLimited", retryAfter: rl.retry_after,
    });
  }

  // 5/6/7. The sanitize + repack pipeline lives in the BundleProcessor DO
  // (§5). The DO is keyed on device fingerprint. We forward the multipart
  // body verbatim; the DO returns `{sanitized_bundle, canonical_hash, ...}`
  // on success. If the new canonical_hash equals the existing row's
  // content_hash, this is a content-noop update → return {status:"unchanged"}
  // without replacing the R2 blob.
  const doId = env.BUNDLE_PROCESSOR.idFromName(dfHex);
  const stub = env.BUNDLE_PROCESSOR.get(doId);
  const doRes = await stub.fetch("https://do.internal/sanitize", {
    method: "POST",
    headers: c.req.raw.headers,
    body,
  });

  // While W3T9 is in-flight the DO still returns 501; surface the response
  // verbatim in that case so integration tests see a stable error envelope
  // instead of a partially-updated row.
  if (doRes.status >= 400) {
    const text = await doRes.text();
    return new Response(text, {
      status: doRes.status,
      headers: { "content-type": doRes.headers.get("content-type") ?? "application/json" },
    });
  }

  const doBody = (await doRes.json()) as {
    canonical_hash: string;
    sanitized_bundle_key?: string;
    thumbnail_key?: string;
    manifest?: { version?: string };
  };

  if (doBody.canonical_hash === row.content_hash) {
    return new Response(
      JSON.stringify({ artifact_id: id, content_hash: row.content_hash, status: "unchanged" }),
      { status: 200, headers: { "content-type": "application/json; charset=utf-8" } },
    );
  }

  // Replace R2 blob (old hash) and update D1 row.
  const now = Math.floor(Date.now() / 1000);
  const newVersion = doBody.manifest?.version ?? row.version;
  await Promise.all([
    env.BLOBS.delete(`bundles/${row.content_hash}.omnipkg`),
    env.META.prepare(
      `UPDATE artifacts
         SET content_hash = ?, version = ?, updated_at = ?
       WHERE id = ?`,
    ).bind(doBody.canonical_hash, newVersion, now, id).run(),
  ]);

  return new Response(
    JSON.stringify({
      artifact_id: id,
      content_hash: doBody.canonical_hash,
      r2_url: `/v1/download/${id}`,
      thumbnail_url: `/v1/thumbnail/${row.thumbnail_hash}`,
      created_at: row.created_at,
      updated_at: now,
      status: "updated",
    }),
    { status: 200, headers: { "content-type": "application/json; charset=utf-8" } },
  );
});

// ---------------------------------------------------------------------------
// DELETE /v1/artifact/:id  (author-only soft delete)
// ---------------------------------------------------------------------------

app.delete("/:id", async (c) => {
  const env = c.env;
  const id = c.req.param("id");

  // DELETE has an empty body; JWS binds over the empty-string body hash.
  let authed;
  try {
    authed = await verifyJws(c.req.raw, env, new ArrayBuffer(0));
  } catch (e) {
    if (e instanceof AuthError) return errorFromKind("Auth", e.detail, e.message);
    throw e;
  }

  const row = await loadArtifact(env, id);
  if (!row) return errorFromKind("Malformed", "NotFound", "artifact not found");

  const rowPub = new Uint8Array(row.author_pubkey);
  if (!bytesEqual(rowPub, authed.pubkey)) {
    return errorFromKind("Auth", "Forbidden", "not the author of this artifact");
  }

  // Soft-delete: mark removed; keep install_daily history intact.
  await env.META.prepare(
    `UPDATE artifacts SET is_removed = 1, updated_at = ? WHERE id = ?`,
  )
    .bind(Math.floor(Date.now() / 1000), id)
    .run();

  // Best-effort R2 cleanup — failure is not fatal; the tombstone is the
  // source of truth and the blob will be GC'd by a later admin sweep.
  try {
    await env.BLOBS.delete(`bundles/${row.content_hash}.omnipkg`);
  } catch {
    /* swallow */
  }

  return new Response(null, { status: 204 });
});

export default app;
