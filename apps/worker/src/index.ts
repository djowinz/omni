import { Hono } from 'hono';
import type { AppEnv } from './types';
import { errorResponse } from './lib/errors';
import { makeDebugLog } from './lib/debug-log';
import admin from './routes/admin';
import artifact from './routes/artifact';
import config from './routes/config';
import download from './routes/download';
import gallery from './routes/gallery';
import list from './routes/list';
import report from './routes/report';
import thumbnail from './routes/thumbnail';
import upload from './routes/upload';

export { BundleProcessor } from './do/bundle_processor';

const app = new Hono<AppEnv>();

const SEMVER_RE = /^\d+\.\d+\.\d+$/;

/**
 * Per-isolate request counter used by the top-level access log. Surviving
 * across requests within a single Worker isolate makes runaway-loop bugs
 * obvious: a healthy session prints `#1, #2, #3` over seconds; a stuck
 * frontend polling loop prints `#42, #43, #44…` in milliseconds against the
 * same path. Resets on cold start, which is fine — the absolute number isn't
 * meaningful, only the cadence and the (path, method) tuple are.
 */
let __reqSeq = 0;

/**
 * Top-level access log (gated by `OMNI_DEBUG=1`). Logs every incoming
 * request before any route runs so a continuously-firing client surfaces
 * regardless of which sub-route it hits. Diagnostic for "the frontend is
 * hammering the worker" — count + path tells you exactly which hook/effect
 * is the culprit.
 */
app.use('*', async (c, next) => {
  // `c.env` is undefined when tests call `app.request()` directly without
  // a Miniflare runtime; pass an empty object so makeDebugLog no-ops cleanly
  // instead of throwing on `env.OMNI_DEBUG` access.
  const debugLog = makeDebugLog(c.env ?? {});
  const seq = ++__reqSeq;
  const url = new URL(c.req.url);
  const ua = c.req.header('User-Agent') ?? '?';
  debugLog(
    `[req #${seq}] ${c.req.method} ${url.pathname}${url.search} ua=${ua.slice(0, 60)}`,
  );
  await next();
  debugLog(`[req #${seq}] ${c.req.method} ${url.pathname} → ${c.res.status}`);
});

/**
 * Global client-version gate (plan #008 W4T14, contract §2).
 *
 * Every authed request must carry `X-Omni-Version` (semver) and
 * `X-Omni-Sanitize-Version` so the Worker can reason about client capability
 * ahead of JWS verify. Missing/malformed → 400 Malformed.
 *
 * Exemptions (documented inline — any change needs a contract update):
 *   - `GET /v1/config/*` — unauthenticated discovery endpoints used to
 *     bootstrap the client before it knows which versions to advertise
 *     (spec §4a/4b). Applying the gate here would deadlock bootstrap.
 *   - `GET /v1/download/*` — unauthenticated CDN-cacheable downloads (spec §6).
 *     Edge caches strip arbitrary headers; requiring them here would defeat
 *     caching and the anon-install flow.
 *   - `GET /v1/thumbnail/*` — unauthenticated PNG serving. Content-addressed
 *     by SHA-256 so long-cacheable; same exemption rationale as download.
 */
app.use('*', async (c, next) => {
  const path = new URL(c.req.url).pathname;
  if (path.startsWith('/v1/config/')) return next();
  if (path.startsWith('/v1/download/') && c.req.method === 'GET') return next();
  if (path.startsWith('/v1/thumbnail/') && c.req.method === 'GET') return next();

  const version = c.req.header('X-Omni-Version');
  const saniVer = c.req.header('X-Omni-Sanitize-Version');
  if (!version || !SEMVER_RE.test(version)) {
    return errorResponse(400, 'BAD_REQUEST', 'missing/invalid X-Omni-Version', {
      kind: 'Malformed',
      detail: 'BadRequest',
    });
  }
  if (!saniVer) {
    return errorResponse(400, 'BAD_REQUEST', 'missing X-Omni-Sanitize-Version', {
      kind: 'Malformed',
      detail: 'BadRequest',
    });
  }
  await next();
});

app.route('/v1/upload', upload);
app.route('/v1/download', download);
app.route('/v1/thumbnail', thumbnail);
app.route('/v1/list', list);
app.route('/v1/artifact', artifact);
app.route('/v1/config', config);
app.route('/v1/report', report);
app.route('/v1/me/gallery', gallery);
app.route('/v1/admin', admin);

app.notFound(() => errorResponse(404, 'NOT_FOUND', 'no route matched'));

app.onError((err) => {
  const message = err instanceof Error ? err.message : String(err);
  return errorResponse(500, 'SERVER_ERROR', message);
});

export default app;
