/**
 * GET /v1/list — spec #008 §7, contract §4.3.
 *
 * Keyset-paginated artifact listing. Sort modes: `new` (created_at DESC),
 * `installs` (install_count DESC), `name` (name ASC). Tag filtering by
 * substring match against the tags JSON-array column. Cursor is the opaque
 * `{t, i}` tuple encoded by `src/lib/cursor.ts` — clients treat as opaque.
 *
 * Unauthenticated route (§4.3). Edge-cacheable: `Cache-Control: public, max-age=60`.
 */
import { Hono } from 'hono';
import type { AppEnv } from '../types';
import { errorFromKind } from '../lib/errors';
import { encodeCursor, decodeCursor, type Cursor } from '../lib/cursor';
import { makeDebugLog } from '../lib/debug-log';
import { hexEncode } from '../lib/hex';

const app = new Hono<AppEnv>();

type SortMode = 'new' | 'installs' | 'name';

const DEFAULT_LIMIT = 25;
const MAX_LIMIT = 100;
const MIN_LIMIT = 1;

interface ArtifactRow {
  id: string;
  author_pubkey: ArrayBuffer;
  name: string;
  kind: string;
  content_hash: string;
  thumbnail_hash: string;
  tags: string | null;
  install_count: number;
  created_at: number;
  updated_at: number;
}

interface ListItem {
  artifact_id: string;
  name: string;
  kind: string;
  tags: string[];
  installs: number;
  updated_at: number;
  created_at: number;
  author_pubkey: string;
  author_fingerprint_hex: string;
  thumbnail_url: string;
  content_hash: string;
}

function parseTagsColumn(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) return parsed.filter((t): t is string => typeof t === 'string');
  } catch {
    /* column may be a plain csv in legacy rows */
  }
  return [];
}

/**
 * The author fingerprint as used by the host UI is a truncated hex of the
 * pubkey (BIP-39 / emoji renderings happen client-side). Worker emits the
 * first 12 hex chars as `author_fingerprint_hex` per contract §4.3. The
 * client owns longer renderings — invariant #11 (host/client own formatting).
 */
function fingerprintHex(pubkeyHex: string): string {
  return pubkeyHex.slice(0, 12);
}

function rowToItem(row: ArtifactRow): ListItem {
  const pubHex = hexEncode(new Uint8Array(row.author_pubkey));
  return {
    artifact_id: row.id,
    name: row.name,
    kind: row.kind,
    tags: parseTagsColumn(row.tags),
    installs: row.install_count,
    updated_at: row.updated_at,
    created_at: row.created_at,
    author_pubkey: pubHex,
    author_fingerprint_hex: fingerprintHex(pubHex),
    thumbnail_url: `/v1/thumbnail/${row.thumbnail_hash}`,
    content_hash: row.content_hash,
  };
}

function sortModeFrom(raw: string | undefined): SortMode {
  if (raw === 'installs' || raw === 'name') return raw;
  return 'new';
}

function clampLimit(raw: string | undefined): number {
  if (raw === undefined) return DEFAULT_LIMIT;
  const n = parseInt(raw, 10);
  if (!Number.isFinite(n)) return DEFAULT_LIMIT;
  return Math.min(MAX_LIMIT, Math.max(MIN_LIMIT, n));
}

/** Keyset predicate for the given sort mode. Returns SQL fragment + binds. */
function keysetPredicate(sort: SortMode, cur: Cursor): { sql: string; binds: unknown[] } {
  switch (sort) {
    case 'new':
      // ORDER BY created_at DESC, id ASC  →  strictly after (t, i) means
      // (created_at < t) OR (created_at = t AND id > i)
      return {
        sql: 'AND (created_at < ? OR (created_at = ? AND id > ?))',
        binds: [Number(cur.t), Number(cur.t), cur.i],
      };
    case 'installs':
      return {
        sql: 'AND (install_count < ? OR (install_count = ? AND id > ?))',
        binds: [Number(cur.t), Number(cur.t), cur.i],
      };
    case 'name':
      // ORDER BY name ASC, id ASC  →  strictly after (t, i) means
      // (name > t) OR (name = t AND id > i)
      return {
        sql: 'AND (name > ? OR (name = ? AND id > ?))',
        binds: [String(cur.t), String(cur.t), cur.i],
      };
  }
}

function orderClause(sort: SortMode): string {
  switch (sort) {
    case 'new':
      return 'ORDER BY created_at DESC, id ASC';
    case 'installs':
      return 'ORDER BY install_count DESC, id ASC';
    case 'name':
      return 'ORDER BY name ASC, id ASC';
  }
}

function cursorValueForRow(row: ArtifactRow, sort: SortMode): number | string {
  switch (sort) {
    case 'new':
      return row.created_at;
    case 'installs':
      return row.install_count;
    case 'name':
      return row.name;
  }
}

app.get('/', async (c) => {
  const env = c.env;
  const debugLog = makeDebugLog(env);
  const url = new URL(c.req.url);
  const kindParam = url.searchParams.get('kind') ?? undefined;
  const sort = sortModeFrom(url.searchParams.get('sort') ?? undefined);
  const cursorRaw = url.searchParams.get('cursor') ?? undefined;
  const limit = clampLimit(url.searchParams.get('limit') ?? undefined);
  const tagParams = url.searchParams.getAll('tag');
  const rid = Date.now().toString(36);
  const tag = `[list rid=${rid}]`;
  debugLog(
    `${tag} START kind=${kindParam ?? '(all)'} sort=${sort} limit=${limit} tags=${JSON.stringify(tagParams)} cursor=${cursorRaw ? 'present' : 'none'}`,
  );

  const conditions: string[] = ['is_removed = 0'];
  const binds: unknown[] = [];

  if (kindParam && kindParam !== 'all') {
    conditions.push('kind = ?');
    binds.push(kindParam);
  }

  for (const t of tagParams) {
    // tags column is JSON text ['foo','bar'] — LIKE match on the quoted form
    // is sufficient for an overlay utility's low-volume tag-filter path.
    conditions.push(`tags LIKE ?`);
    binds.push(`%"${t}"%`);
  }

  const authorPubkeyRaw = url.searchParams.get('author_pubkey') ?? undefined;
  if (authorPubkeyRaw !== undefined) {
    if (!/^[0-9a-fA-F]{64}$/.test(authorPubkeyRaw)) {
      return errorFromKind('Malformed', 'BadRequest', 'author_pubkey must be 64-hex');
    }
    // SQL BLOB literal: X'<hex>' matches the ArrayBuffer column bytewise.
    conditions.push(`author_pubkey = X'${authorPubkeyRaw.toLowerCase()}'`);
    // No bind push — hex is validated by the regex above and inlined into the SQL
    // as a BLOB literal, which SQLite parses safely.
  }

  let cursorSql = '';
  if (cursorRaw) {
    let cur: Cursor;
    try {
      cur = decodeCursor(cursorRaw);
    } catch {
      return errorFromKind('Malformed', 'BadRequest', 'cursor is malformed');
    }
    const pred = keysetPredicate(sort, cur);
    cursorSql = ` ${pred.sql}`;
    binds.push(...pred.binds);
  }

  const whereSql = conditions.length > 0 ? `WHERE ${conditions.join(' AND ')}` : '';
  const sql = `
    SELECT id, author_pubkey, name, kind, content_hash, thumbnail_hash,
           tags, install_count, created_at, updated_at
    FROM artifacts
    ${whereSql}${cursorSql}
    ${orderClause(sort)}
    LIMIT ?
  `;

  // Fetch one extra row to detect "has next page" without a COUNT query.
  binds.push(limit + 1);

  debugLog(`${tag} sql=${sql.replace(/\s+/g, ' ').trim()}`);
  debugLog(`${tag} binds=${JSON.stringify(binds)}`);
  const { results } = await c.env.META.prepare(sql)
    .bind(...binds)
    .all<ArtifactRow>();

  const rows = results ?? [];
  debugLog(`${tag} rows returned: ${rows.length}`);
  const hasMore = rows.length > limit;
  const page = hasMore ? rows.slice(0, limit) : rows;
  const items = page.map(rowToItem);
  debugLog(`${tag} serialized ${items.length} items${hasMore ? ' + next_cursor' : ''}`);

  const body: { items: ListItem[]; next_cursor?: string } = { items };
  if (hasMore) {
    const last = page[page.length - 1]!;
    body.next_cursor = encodeCursor({ t: cursorValueForRow(last, sort), i: last.id });
  }

  return new Response(JSON.stringify(body), {
    status: 200,
    headers: {
      'content-type': 'application/json; charset=utf-8',
      'Cache-Control': 'public, max-age=60',
    },
  });
});

export default app;
