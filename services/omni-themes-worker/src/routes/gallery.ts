/**
 * GET /v1/me/gallery — spec #008 §12, contract §4.8.
 *
 * Authed. Returns every non-removed artifact authored by the caller, sorted
 * updated_at DESC. No pagination — the `upload_new` daily quota (5/day) caps
 * authors at a low-enough count that a full scan is cheap.
 */
import { Hono } from "hono";
import type { AppEnv } from "../types";
import { errorFromKind } from "../lib/errors";
import { verifyJws, AuthError } from "../lib/auth";
import { hexEncode } from "../lib/hex";

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

app.get("/", async (c) => {
  const env = c.env;
  const req = c.req.raw;

  let authed;
  try {
    authed = await verifyJws(req, env, new ArrayBuffer(0));
  } catch (e) {
    if (e instanceof AuthError) return errorFromKind("Auth", e.detail, e.message);
    throw e;
  }

  const pubHex = hexEncode(authed.pubkey);

  const { results } = await env.META.prepare(
    `SELECT id, name, kind, tags, content_hash, thumbnail_hash,
            install_count, created_at, updated_at, is_removed
     FROM artifacts
     WHERE author_pubkey = ? AND is_removed = 0
     ORDER BY updated_at DESC`,
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
    thumbnail_url: `/v1/thumbnail/${row.thumbnail_hash}`,
    content_hash: row.content_hash,
  }));

  return new Response(JSON.stringify({ items }), {
    status: 200,
    headers: { "content-type": "application/json; charset=utf-8" },
  });
});

export default app;
