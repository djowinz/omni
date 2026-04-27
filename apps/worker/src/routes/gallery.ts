/**
 * GET /v1/me/gallery — spec #008 §12, contract §4.8.
 *
 * Authed. Returns every non-removed artifact authored by the caller, sorted
 * updated_at DESC. No pagination — the `upload_new` daily quota (5/day) caps
 * authors at a low-enough count that a full scan is cheap.
 */
import { Hono } from 'hono';
import type { AppEnv } from '../types';
import { errorFromKind } from '../lib/errors';
import { verifyJws, AuthError } from '../lib/auth';
import { hexEncode } from '../lib/hex';
import { makeDebugLog } from '../lib/debug-log';

const app = new Hono<AppEnv>();

interface GalleryRow {
  id: string;
  name: string;
  kind: string;
  tags: string | null;
  content_hash: string;
  thumbnail_hash: string;
  install_count: number;
  created_at: number;
  updated_at: number;
  is_removed: number;
  // LEFT JOIN authors per identity-completion-and-display-name spec §4.4
  // (OWI-79). Always the caller's own display_name (gallery is scoped to the
  // JWS kid pubkey); NULL until the caller has set one.
  author_display_name: string | null;
}

function parseTags(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const p = JSON.parse(raw);
    if (Array.isArray(p)) return p.filter((t): t is string => typeof t === 'string');
  } catch {
    /* fall through */
  }
  return [];
}

app.get('/', async (c) => {
  const env = c.env;
  const req = c.req.raw;
  makeDebugLog(env)(`[gallery] GET /v1/me/gallery`);

  let authed;
  try {
    authed = await verifyJws(req, env, new ArrayBuffer(0));
  } catch (e) {
    if (e instanceof AuthError) return errorFromKind('Auth', e.detail, e.message);
    throw e;
  }

  const pubHex = hexEncode(authed.pubkey);

  // LEFT JOIN authors per identity-completion-and-display-name spec §4.4
  // (OWI-79): gallery is scoped to the JWS kid pubkey, so the JOIN always
  // resolves to the caller's own authors row (or NULL display_name if they
  // haven't called identity.setDisplayName yet). The column refs use
  // `artifacts.` prefixes because `id` and `created_at` collide with the
  // authors table columns (authors has its own pubkey PK + created_at).
  const { results } = await env.META.prepare(
    `SELECT artifacts.id, artifacts.name, artifacts.kind, artifacts.tags,
            artifacts.content_hash, artifacts.thumbnail_hash,
            artifacts.install_count, artifacts.created_at, artifacts.updated_at,
            artifacts.is_removed,
            authors.display_name AS author_display_name
     FROM artifacts
     LEFT JOIN authors ON authors.pubkey = artifacts.author_pubkey
     WHERE artifacts.author_pubkey = ? AND artifacts.is_removed = 0
     ORDER BY artifacts.updated_at DESC`,
  )
    .bind(authed.pubkey)
    .all<GalleryRow>();

  const items = (results ?? []).map((row) => ({
    artifact_id: row.id,
    name: row.name,
    kind: row.kind,
    tags: parseTags(row.tags),
    installs: row.install_count,
    updated_at: row.updated_at,
    created_at: row.created_at,
    author_pubkey: pubHex,
    author_fingerprint_hex: pubHex.slice(0, 12),
    author_display_name: row.author_display_name ?? null,
    thumbnail_url: `/v1/thumbnail/${row.thumbnail_hash}`,
    content_hash: row.content_hash,
  }));

  return new Response(JSON.stringify({ items }), {
    status: 200,
    headers: { 'content-type': 'application/json; charset=utf-8' },
  });
});

export default app;
