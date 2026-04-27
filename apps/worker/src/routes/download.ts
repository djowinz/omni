/**
 * GET /v1/download/:artifact_id — spec #008 §6.
 *
 * 1. Look up artifact; 404 on miss.
 * 2. If is_removed = 1, 410 TOMBSTONED.
 * 3. Fetch R2 blob keyed by content_hash.
 * 4. Rate limit (`download`): authed → df claim; unauthed → cf-connecting-ip.
 * 5. Atomic install_count + install_daily upsert.
 * 6. Return bytes with contract headers (X-Omni-Content-Hash,
 *    X-Omni-Author-Pubkey, X-Omni-Manifest). X-Omni-Signature is omitted
 *    when the stored blob has no exposed signature (Worker-repacked blobs
 *    are stripped per invariant #1; current WASM surface also does not expose
 *    the signature for natively-signed blobs — see step 6 for details).
 *
 * Auth is optional on download (edge-cacheable).
 */
import { Hono } from 'hono';
import type { AppEnv } from '../types';
import { errorResponse, errorFromKind } from '../lib/errors';
import { verifyJws, AuthError } from '../lib/auth';
import { checkAndIncrement } from '../lib/rate_limit';
import { loadWasm } from '../lib/wasm';
import { hexEncode } from '../lib/hex';
import { makeDebugLog } from '../lib/debug-log';

const app = new Hono<AppEnv>();

app.get('/:id', async (c) => {
  const env = c.env;
  const req = c.req.raw;
  const id = c.req.param('id');
  const debugLog = makeDebugLog(env);
  debugLog(`[download] GET /v1/download/${id}`);

  // 1. Lookup
  const row = await env.META.prepare(
    `SELECT id, author_pubkey, content_hash, thumbnail_hash, is_removed
     FROM artifacts WHERE id = ?`,
  )
    .bind(id)
    .first<{
      id: string;
      author_pubkey: ArrayBuffer;
      content_hash: string;
      thumbnail_hash: string;
      is_removed: number;
    }>();
  if (!row) return errorFromKind('Malformed', 'NotFound', 'artifact not found');
  if (row.is_removed) return errorFromKind('Integrity', 'Tombstoned', 'artifact tombstoned');

  // 4. Rate-limit — authed path uses df claim; unauthed uses IP.
  const authHeader = req.headers.get('authorization') ?? req.headers.get('Authorization');
  let dfHex = '';
  let pubHex = '';
  if (authHeader) {
    // Need body for JWS verify; download is GET with empty body.
    try {
      const a = await verifyJws(req, env, new ArrayBuffer(0));
      dfHex = hexEncode(a.device_fp);
      pubHex = hexEncode(a.pubkey);
    } catch (e) {
      if (e instanceof AuthError) return errorFromKind('Auth', e.detail, e.message);
      throw e;
    }
  } else {
    const ip = req.headers.get('cf-connecting-ip') ?? 'unknown';
    dfHex = `ip_${ip}`;
  }
  const rl = await checkAndIncrement(env, dfHex, pubHex, 'download');
  if (!rl.allowed) {
    return errorResponse(429, 'RATE_LIMITED', 'download rate limit exceeded', {
      kind: 'Quota',
      detail: 'RateLimited',
      retryAfter: rl.retry_after,
    });
  }

  // 3. Fetch blob.
  const obj = await env.BLOBS.get(`bundles/${row.content_hash}.omnipkg`);
  if (!obj) return errorFromKind('Io', undefined, 'bundle blob missing from storage');
  const bytes = await obj.arrayBuffer();

  // 5. Increment counters (atomic on D1; install_daily upserted for today).
  const day = new Date().toISOString().slice(0, 10); // YYYY-MM-DD UTC
  await env.META.batch([
    env.META.prepare('UPDATE artifacts SET install_count = install_count + 1 WHERE id = ?').bind(
      id,
    ),
    env.META.prepare(
      `INSERT INTO install_daily (artifact_id, day, install_count)
       VALUES (?, ?, 1)
       ON CONFLICT(artifact_id, day) DO UPDATE SET install_count = install_count + 1`,
    ).bind(id, day),
  ]);

  // 6. Extract manifest via the unsigned fast path (`bundle.unpackManifest`).
  // This works for BOTH signed and Worker-repacked (stripped-JWS) blobs —
  // `identity.unpackSignedBundle` would throw on the stripped-JWS case, which
  // is the dominant path (invariant #1: the Worker cannot re-sign sanitized
  // bundles). Per architectural invariant #19b the manifest-only fast path
  // skips decompression of file entries entirely.
  //
  // X-Omni-Signature: the current WASM surface (`WasmSignedBundleHandle`) does
  // NOT expose the signature bytes — only `manifest()`, `authorPubkey()`, and
  // `nextFile()`. Per contract §4.2 the header must carry the real Ed25519
  // signature over content bytes or be absent entirely; we never emit an
  // empty-string or placeholder. For Worker-repacked blobs there is no
  // signature available at all (JWS was stripped during sanitize). Future: if
  // we need to expose the signature for the signed path, add a
  // `signatureBytes()` accessor to `WasmSignedBundleHandle` in
  // `crates/identity/src/wasm.rs` and conditionally emit the header here.
  let manifestB64 = '';
  try {
    const { bundle } = await loadWasm();
    const manifestJson = bundle.unpackManifest(new Uint8Array(bytes), undefined);
    manifestB64 = btoa(JSON.stringify(manifestJson));
  } catch {
    // Non-fatal: proceed without optional header. The blob may predate the
    // current schema or be otherwise unreadable — the integrity gate is the
    // X-Omni-Content-Hash header, which is always emitted.
  }

  const authorPubHex = hexEncode(new Uint8Array(row.author_pubkey));

  const headers: Record<string, string> = {
    'content-type': 'application/octet-stream',
    'X-Omni-Content-Hash': row.content_hash,
    'X-Omni-Author-Pubkey': authorPubHex,
    'Cache-Control': 'public, max-age=60',
  };
  if (manifestB64) headers['X-Omni-Manifest'] = manifestB64;
  // X-Omni-Signature intentionally omitted — see comment above.

  return new Response(bytes, { status: 200, headers });
});

export default app;
