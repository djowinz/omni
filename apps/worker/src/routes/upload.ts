/**
 * POST /v1/upload — spec #008 §4.
 *
 * Pipeline (1..14):
 *   1. Body cap (config:limits.max_bundle_compressed, 60s cached)
 *   2. Buffer body, verify JWS auth
 *   3. Rate-limit (upload_new / upload_update / upload_new_bundle)
 *   4. Multipart parse
 *   5. Manifest-only fast path (omni_bundle.unpackManifest via identity handle)
 *   6. Content signature verify (omni_identity.unpackSignedBundle) + kid match
 *   7. Tag vocab validation (suggested_alternatives)
 *   8. Sanitize path (always via BundleProcessor DO — post-sanitize manifest
 *      file-hash rewriting is required because sanitizers mutate bytes; the DO
 *      centralises the rehash+repack. There is no inline fast path.)
 *   9. canonical_hash(manifest)
 *  10. Dedup/tombstone
 *  11. Per-author name uniqueness
 *  12. Persist R2 + D1
 *  13. (never bump install_daily here)
 *  14. Respond
 */
import { Hono } from 'hono';
import type { AppEnv } from '../types';
import type { Env } from '../env';
import {
  errorResponse,
  errorFromKind,
  classifyWasmError,
  authorNameConflictResponse,
} from '../lib/errors';
import { verifyJws, AuthError } from '../lib/auth';
import { checkAndIncrement, type RateLimitAction } from '../lib/rate_limit';
import { parseMultipart, MultipartError } from '../lib/multipart';
import { loadWasm } from '../lib/wasm';
import { canonicalHash } from '../lib/canonical';
import { sanitizeViaDO } from '../lib/sanitize';
import { hexEncode } from '../lib/hex';
import { makeDebugLog } from '../lib/debug-log';
import { validateDisplayName } from '../lib/display_name';

const app = new Hono<AppEnv>();

// ---------- Helpers ---------------------------------------------------------

interface Limits {
  max_bundle_compressed: number;
  max_bundle_uncompressed: number;
  max_entries: number;
  version: number;
  updated_at: number;
}

/**
 * Config KV reads. Fail-closed: if `config:limits` is missing, surface a 500
 * so a misconfigured deployment can't silently accept uploads against stale
 * hardcoded defaults. Matches the pattern `config.ts` + `admin.ts` use. The
 * KV read is on every request (low-volume service, invariant #0); a per-
 * isolate TTL cache lives in `config.ts` for public read paths.
 */
async function getLimits(env: Env): Promise<Limits | Response> {
  const raw = await env.STATE.get('config:limits');
  if (raw === null) {
    return errorResponse(500, 'SERVER_ERROR', 'config:limits not seeded', {
      kind: 'Io',
    });
  }
  return JSON.parse(raw) as Limits;
}

async function getVocab(env: Env) {
  const raw = await env.STATE.get('config:vocab');
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
  let i = 0,
    j = 0,
    skipped = 0;
  while (i < s.length && j < t.length) {
    if (s[i] === t[j]) {
      i++;
      j++;
    } else {
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
  const lenHeader = req.headers.get('content-length');
  if (lenHeader !== null) {
    const n = Number(lenHeader);
    if (Number.isFinite(n) && n > cap) return { over: true };
  }
  // Stream-read with running total check (covers missing/lying content-length).
  if (req.body) {
    const reader = req.body.getReader();
    const chunks: Uint8Array[] = [];
    let total = 0;
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (value) {
        total += value.byteLength;
        if (total > cap) {
          try {
            await reader.cancel();
          } catch {
            /* swallow */
          }
          return { over: true };
        }
        chunks.push(value);
      }
    }
    const out = new Uint8Array(total);
    let off = 0;
    for (const c of chunks) {
      out.set(c, off);
      off += c.byteLength;
    }
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
  // OWI-59: WASM (`apps/worker/src/lib/wasm.ts`) returns this as a JS `Map`
  // (serde-wasm-bindgen serializes Rust `BTreeMap` as `Map`). Tests and any
  // future plain-JSON callsites pass `Record`. `isThemeOnly` handles both.
  resource_kinds?: Record<string, unknown> | Map<string, unknown>;
  [k: string]: unknown;
}

/**
 * Classify a manifest as theme-only for rate-limit-bucket selection.
 *
 * Returns `true` ONLY when `resource_kinds` is a non-empty container whose
 * every key is `"theme"`. Missing / null / non-object / empty
 * `resource_kinds` returns `false` — the caller (host) is responsible for
 * populating `resource_kinds`; absence is NOT theme-only-by-default (would
 * mis-bucket non-theme bundles into the lighter quota).
 *
 * Pre-2026-04-25 behavior was the inverse on the missing/empty branches and
 * silently biased every host that hadn't populated `resource_kinds` into the
 * theme bucket. The host fix in OWI-33 (Task A0.7) populates `resource_kinds`
 * from bundle contents so all bundles classify correctly.
 *
 * Map vs Object (OWI-59 / Task A1.7): the WASM `unpackSignedBundle` returns
 * a manifest whose `resource_kinds` is a JS `Map`, not a plain object —
 * `serde-wasm-bindgen` (used by `apps/worker/src/lib/wasm.ts`) serializes
 * Rust `BTreeMap<String, _>` as a `Map`. `Object.keys(map)` returns `[]`
 * for any `Map`, so without explicit Map handling every real upload
 * misclassified as not-theme-only and got billed against `upload_new_bundle`
 * (3/day) instead of `upload_new` (5/day). Handle both shapes so test-time
 * plain-object fixtures and runtime WASM Maps both classify correctly.
 *
 * Exported for direct unit testing — see `apps/worker/test/upload-is-theme-only.test.ts`.
 */
export function isThemeOnly(manifest: Manifest): boolean {
  const kinds = manifest.resource_kinds;
  if (!kinds || typeof kinds !== 'object') return false;
  const keys: string[] =
    kinds instanceof Map
      ? Array.from((kinds as Map<string, unknown>).keys())
      : Object.keys(kinds as Record<string, unknown>);
  if (keys.length === 0) return false;
  return keys.every((k) => k === 'theme');
}

// ---------- Route -----------------------------------------------------------

app.post('/', async (c) => {
  const env = c.env;
  const debugLog = makeDebugLog(env);
  const req = c.req.raw;
  const url = new URL(req.url);
  // Request id for correlating multi-stage log lines. ms-epoch is good enough
  // for dev tailing — we only need to tell concurrent requests apart.
  const rid = Date.now().toString(36);
  const tag = `[upload rid=${rid}]`;
  debugLog(
    `${tag} START method=${req.method} url=${url.pathname}${url.search} content-length=${req.headers.get('content-length') ?? '(none)'}`,
  );

  // Step 1 — body cap
  debugLog(`${tag} step 1: getLimits + body cap`);
  const limitsOrErr = await getLimits(env);
  if (limitsOrErr instanceof Response) {
    debugLog(`${tag} FAIL step 1: getLimits returned ${limitsOrErr.status}`);
    return limitsOrErr;
  }
  const limits = limitsOrErr;
  debugLog(
    `${tag}   limits.max_bundle_compressed=${limits.max_bundle_compressed} max_entries=${limits.max_entries}`,
  );
  const bodyOrOver = await readBodyWithCap(req, limits.max_bundle_compressed);
  if ('over' in (bodyOrOver as object) && (bodyOrOver as { over: true }).over) {
    debugLog(`${tag} FAIL step 1: body over cap`);
    return errorFromKind('Malformed', 'SizeExceeded', 'request body exceeds max_bundle_compressed');
  }
  const body = bodyOrOver as ArrayBuffer;
  debugLog(`${tag}   body buffered: ${body.byteLength} bytes`);

  // Step 2 — auth
  debugLog(`${tag} step 2: verifyJws`);
  let auth;
  try {
    auth = await verifyJws(req, env, body);
  } catch (e) {
    if (e instanceof AuthError) {
      debugLog(`${tag} FAIL step 2: AuthError detail=${e.detail} message=${e.message}`);
      return errorFromKind('Auth', e.detail, e.message);
    }
    debugLog(`${tag} FAIL step 2: uncaught ${(e as Error)?.message ?? String(e)}`);
    throw e;
  }

  const pubkeyHex = hexEncode(auth.pubkey);
  const dfHex = hexEncode(auth.device_fp);
  debugLog(`${tag}   auth ok: pk=${pubkeyHex.slice(0, 10)}… df=${dfHex.slice(0, 10)}…`);

  // Step 3 — rate limit. `artifact_id` as query param signals update path.
  const isUpdate = url.searchParams.has('artifact_id');
  const action: RateLimitAction = isUpdate ? 'upload_update' : 'upload_new';
  debugLog(`${tag} step 3: rate limit action=${action} isUpdate=${isUpdate}`);
  const rl = await checkAndIncrement(env, dfHex, pubkeyHex, action);
  if (!rl.allowed) {
    if (rl.turnstile) {
      debugLog(`${tag} FAIL step 3: turnstile required`);
      return errorFromKind('Quota', 'TurnstileRequired', 'turnstile challenge required');
    }
    debugLog(`${tag} FAIL step 3: rate limited, retry_after=${rl.retry_after}s`);
    return errorResponse(429, 'RATE_LIMITED', 'rate limit exceeded', {
      kind: 'Quota',
      detail: 'RateLimited',
      retryAfter: rl.retry_after,
    });
  }

  // Step 4 — parse multipart. Hono's `c.req.raw.formData()` is single-shot; we
  // already buffered `body`, so re-synthesize a Request with the buffered body.
  debugLog(`${tag} step 4: parseMultipart`);
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
      debugLog(`${tag} FAIL step 4: MultipartError ${e.message}`);
      return errorFromKind('Malformed', 'BadRequest', `multipart: ${e.message}`);
    }
    throw e;
  }

  // Step 4b — optional `display_name` form field (plan §T5 / spec §4.3).
  //
  // The shared `parseMultipart` helper deliberately returns just bundle +
  // thumbnail (multipart.ts is the contract surface for binary parts). To
  // pull a string field without touching that contract, synthesize a second
  // Request from the same buffered body and let workerd parse the form
  // again. `body` is an ArrayBuffer, so this is just a header/body re-pair —
  // no double-buffer of bytes. Workerd's FormData is built lazily.
  let displayNameValue: string | null = null;
  {
    const formReq2 = new Request(req.url, {
      method: req.method,
      headers: req.headers,
      body,
    });
    let form2: FormData;
    try {
      form2 = await formReq2.formData();
    } catch (e) {
      // The first parseMultipart call would have surfaced a structural
      // failure already; if we land here, treat as malformed and bail.
      debugLog(
        `${tag} FAIL step 4b: re-parse for display_name: ${(e as Error)?.message ?? String(e)}`,
      );
      return errorFromKind(
        'Malformed',
        'BadRequest',
        'multipart: failed to re-parse for display_name',
      );
    }
    const raw = form2.get('display_name');
    if (raw !== null) {
      // FormData fields are either strings (text inputs) or File/Blob
      // (binary parts). For `display_name` we accept only the text form;
      // a binary blob is a client mistake — reject explicitly so the
      // caller learns instead of silently dropping the value.
      if (typeof raw !== 'string') {
        return errorFromKind(
          'Malformed',
          'BadRequest',
          'display_name multipart field must be a text part, not a file',
        );
      }
      const v = validateDisplayName(raw);
      if ('err' in v) {
        debugLog(`${tag} FAIL step 4b: display_name rejected: ${v.err}`);
        return errorFromKind('Malformed', 'BadRequest', v.err);
      }
      displayNameValue = v.ok;
    }
  }

  // Step 5 — manifest fast path
  debugLog(`${tag} step 5: loadWasm + unpackSignedBundle`);
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
    signedHandle = identity.unpackSignedBundle(
      parts.bundle,
      undefined,
    ) as unknown as SignedHandleLike;
    manifest = signedHandle.manifest() as Manifest;
    debugLog(
      `${tag}   manifest ok: name=${manifest.name ?? '(none)'} version=${manifest.version ?? '(none)'} schema=${manifest.schema_version}`,
    );
  } catch (e) {
    const cat = classifyWasmError(e);
    debugLog(`${tag} FAIL step 5: wasm unpack ${cat.kind}/${cat.detail} — ${cat.message}`);
    return errorFromKind(cat.kind, cat.detail, cat.message);
  }

  // Step 6 — content signature verify + kid match (via the signed-bundle's authorPubkey).
  debugLog(`${tag} step 6: kid match check`);
  const authorPub = signedHandle.authorPubkey();
  const authorPubHex = hexEncode(authorPub);
  if (authorPubHex !== pubkeyHex) {
    debugLog(
      `${tag} FAIL step 6: author ${authorPubHex.slice(0, 10)}… != request kid ${pubkeyHex.slice(0, 10)}…`,
    );
    try {
      signedHandle.free?.();
    } catch {
      /* swallow */
    }
    return errorFromKind('Auth', 'Forbidden', 'bundle author pubkey does not match request kid');
  }

  // Step 7 — tag validation
  const vocab = await getVocab(env);
  const tags = Array.isArray(manifest.tags) ? manifest.tags : [];
  for (const t of tags) {
    if (typeof t !== 'string' || !vocab.tags.includes(t)) {
      try {
        (signedHandle as SignedHandleLike | null)?.free?.();
      } catch {
        /* swallow */
      }
      const suggested = typeof t === 'string' ? suggestAlternatives(t, vocab.tags) : [];
      const body: Record<string, unknown> = {
        error: { code: 'MANIFEST_INVALID', message: `unknown tag: ${String(t)}` },
        kind: 'Malformed',
        detail: 'UnknownTag',
        suggested_alternatives: suggested,
      };
      return new Response(JSON.stringify(body), {
        status: 400,
        headers: { 'content-type': 'application/json; charset=utf-8' },
      });
    }
  }

  // Release the handle — the DO re-opens the bundle to iterate files.
  try {
    signedHandle?.free?.();
  } catch {
    /* swallow */
  }
  signedHandle = null;

  // Step 8 — sanitize path. All uploads route through the BundleProcessor DO.
  // Sanitizers mutate file bytes, which invalidates `manifest.files[*].sha256`;
  // the DO centralises the post-sanitize rehash + repack so every upload
  // produces a consistent bundle. There is no inline fast path (spec §4.8
  // 2026-04-15 post-review simplification).
  const themeOnly = isThemeOnly(manifest);

  // Non-theme bundles also count against `upload_new_bundle`.
  if (!themeOnly && !isUpdate) {
    const rl2 = await checkAndIncrement(env, dfHex, pubkeyHex, 'upload_new_bundle');
    if (!rl2.allowed) {
      if (rl2.turnstile) return errorFromKind('Quota', 'TurnstileRequired', 'turnstile required');
      return errorResponse(429, 'RATE_LIMITED', 'bundle rate limit exceeded', {
        kind: 'Quota',
        detail: 'RateLimited',
        retryAfter: rl2.retry_after,
      });
    }
  }

  debugLog(`${tag} step 8: sanitizeViaDO (themeOnly=${themeOnly})`);
  let sanitized;
  try {
    sanitized = await sanitizeViaDO(env, parts.bundle, dfHex, limits);
    debugLog(`${tag}   sanitize ok: ${sanitized.sanitizedBundleBytes.byteLength} bytes out`);
  } catch (e) {
    const cat = classifyWasmError(e);
    debugLog(`${tag} FAIL step 8: sanitize ${cat.kind}/${cat.detail} — ${cat.message}`);
    return errorFromKind(cat.kind, cat.detail, cat.message);
  }

  // Step 9 — canonical hash (native hex)
  debugLog(`${tag} step 9: canonicalHash`);
  const hashBytes = await canonicalHash(manifest);
  const contentHash = hexEncode(hashBytes);
  debugLog(`${tag}   content_hash=${contentHash.slice(0, 16)}…`);

  // Step 10 — dedup / tombstone
  const tombRow = await env.META.prepare(
    'SELECT content_hash FROM tombstones WHERE content_hash = ?',
  )
    .bind(contentHash)
    .first<{ content_hash: string }>();
  if (tombRow) {
    return errorFromKind('Integrity', 'Tombstoned', 'content is tombstoned');
  }
  const dedupRow = await env.META.prepare(
    'SELECT artifact_id FROM content_hashes WHERE content_hash = ?',
  )
    .bind(contentHash)
    .first<{ artifact_id: string }>();
  if (dedupRow) {
    const row = await env.META.prepare(
      'SELECT id, content_hash, thumbnail_hash, created_at FROM artifacts WHERE id = ? AND is_removed = 0',
    )
      .bind(dedupRow.artifact_id)
      .first<{ id: string; content_hash: string; thumbnail_hash: string; created_at: number }>();
    if (row) {
      return Response.json({
        artifact_id: row.id,
        content_hash: row.content_hash,
        // Use the HTTP paths the worker serves (matches /v1/list's rowToItem
        // in routes/list.ts). r2:// pseudo-URLs are internal identifiers
        // for the R2 bucket and are NOT fetchable by Chromium — emitting
        // them caused `net::ERR_UNKNOWN_URL_SCHEME` in the Electron UI
        // when consuming the post-upload cache during an immediate list
        // render.
        r2_url: `/v1/download/${row.id}`,
        thumbnail_url: `/v1/thumbnail/${row.thumbnail_hash}`,
        created_at: row.created_at,
        status: 'deduplicated',
      });
    }
  }

  // Step 11 — per-author name uniqueness (only on new upload).
  //
  // On conflict we surface the structured `AuthorNameConflict` envelope
  // (`apps/worker/src/lib/errors.ts::authorNameConflictResponse`) so the
  // renderer's Step 4 amber recovery card (INV-7.6.3, spec §8.7) can render
  // the existing-artifact summary row and offer Link-and-update.
  const authorPubkeyBlob = auth.pubkey;
  if (!isUpdate) {
    const nameRow = await env.META.prepare(
      'SELECT id, version, updated_at FROM artifacts WHERE author_pubkey = ? AND name = ? AND is_removed = 0',
    )
      .bind(authorPubkeyBlob, manifest.name)
      .first<{ id: string; version: string; updated_at: number }>();
    if (nameRow) {
      return authorNameConflictResponse({
        existing_artifact_id: nameRow.id,
        existing_version: nameRow.version,
        last_published_at: new Date(nameRow.updated_at * 1000).toISOString(),
      });
    }
  }

  // Step 12 — persist R2 + D1
  const thumbHashBuf = await crypto.subtle.digest('SHA-256', parts.thumbnail as BufferSource);
  const thumbHash = hexEncode(new Uint8Array(thumbHashBuf));

  await env.BLOBS.put(`bundles/${contentHash}.omnipkg`, sanitized.sanitizedBundleBytes);
  await env.BLOBS.put(`thumbnails/${thumbHash}.png`, parts.thumbnail);

  const artifactId = crypto.randomUUID();
  const now = Math.floor(Date.now() / 1000);
  const kind = themeOnly ? 'theme' : 'bundle';

  // Author upsert (first-seen). Per spec §4.3 + plan §T5: optional
  // `display_name` from the multipart body lands here. `COALESCE(excluded.
  // display_name, authors.display_name)` realises Q2's C decision —
  // absent on upload preserves prior name (excluded value is NULL); present
  // overwrites. New-author insert binds the validated string (or NULL).
  await env.META.prepare(
    `INSERT INTO authors (pubkey, display_name, created_at, total_uploads, is_new_creator, is_denied)
     VALUES (?, ?, ?, 1, 1, 0)
     ON CONFLICT(pubkey) DO UPDATE SET
       total_uploads = total_uploads + 1,
       display_name  = COALESCE(excluded.display_name, authors.display_name)`,
  )
    .bind(authorPubkeyBlob, displayNameValue, now)
    .run();

  // The D1 `signature` column is a legacy denormalization. The real content
  // signature lives inside the stored `.omnipkg` (the `signature.jws` zip
  // entry) and is not extracted to D1 today. The schema declares this column
  // NOT NULL (pre-launch, no data migration concern), so we write an empty
  // blob as a sentinel — callers MUST NOT treat this as a real signature.
  // When/if a hot path needs it, add a DO-side extraction step in a follow-up
  // and either repopulate this column or add a new `content_signature_jws`
  // column alongside (this field stays empty to avoid silent contract drift).
  const signatureEmpty = new Uint8Array(0);

  await env.META.prepare(
    `INSERT INTO artifacts
     (id, author_pubkey, name, kind, content_hash, thumbnail_hash, description,
      tags, license, version, omni_min_version, signature, created_at, updated_at,
      install_count, report_count, is_removed, is_featured)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, 0, 0)`,
  )
    .bind(
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
      signatureEmpty,
      now,
      now,
    )
    .run();

  await env.META.prepare(
    `INSERT OR IGNORE INTO content_hashes (content_hash, artifact_id, first_seen_at)
     VALUES (?, ?, ?)`,
  )
    .bind(contentHash, artifactId, now)
    .run();

  // Step 14 — respond. Emit HTTP paths (not r2:// pseudo-URLs) so the
  // Electron renderer can fetch the thumbnail immediately — see the
  // matching dedup-path comment above.
  return Response.json({
    artifact_id: artifactId,
    content_hash: contentHash,
    r2_url: `/v1/download/${artifactId}`,
    thumbnail_url: `/v1/thumbnail/${thumbHash}`,
    created_at: now,
    status: 'created',
  });
});

export default app;
