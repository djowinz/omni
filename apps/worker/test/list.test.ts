/**
 * Tier B — Miniflare-backed integration tests for GET /v1/list (W3T11).
 *
 * Covers spec #008 §7 and contract §4.3 / §5:
 *   - sort=new  (created_at DESC tie-broken by id ASC)
 *   - sort=installs  (install_count DESC tie-broken by id ASC)
 *   - sort=name  (name ASC tie-broken by id ASC)
 *   - tag filter (LIKE on JSON-array column)
 *   - keyset cursor round-trip (decodeCursor → next page excludes cursor row)
 *   - limit clamp (min 1, max 100, default 25)
 *   - Cache-Control: public, max-age=60
 */
import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { env, SELF as RAW_SELF } from 'cloudflare:test';

// Inject the W4T14 client-version headers into every Miniflare request.
const SELF = {
  fetch(input: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    if (!headers.has('X-Omni-Version')) headers.set('X-Omni-Version', '0.1.0');
    if (!headers.has('X-Omni-Sanitize-Version')) headers.set('X-Omni-Sanitize-Version', '1');
    return RAW_SELF.fetch(input, { ...init, headers });
  },
};
import type { Env } from '../src/env';
import { decodeCursor } from '../src/lib/cursor';

declare module 'cloudflare:test' {
  interface ProvidedEnv extends Env {}
}

async function ensureSchema(): Promise<void> {
  await env.META.exec(
    `CREATE TABLE IF NOT EXISTS authors (pubkey BLOB PRIMARY KEY, display_name TEXT UNIQUE, created_at INTEGER NOT NULL, total_uploads INTEGER NOT NULL DEFAULT 0, is_new_creator INTEGER NOT NULL DEFAULT 1, is_denied INTEGER NOT NULL DEFAULT 0)`,
  );
  await env.META.exec(
    `CREATE TABLE IF NOT EXISTS artifacts (id TEXT PRIMARY KEY, author_pubkey BLOB NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, content_hash TEXT NOT NULL, thumbnail_hash TEXT NOT NULL, description TEXT, tags TEXT, license TEXT, version TEXT NOT NULL, omni_min_version TEXT NOT NULL, signature BLOB NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, install_count INTEGER NOT NULL DEFAULT 0, report_count INTEGER NOT NULL DEFAULT 0, is_removed INTEGER NOT NULL DEFAULT 0, is_featured INTEGER NOT NULL DEFAULT 0, UNIQUE (author_pubkey, name))`,
  );
  await env.META.exec(
    `CREATE TABLE IF NOT EXISTS content_hashes (content_hash TEXT PRIMARY KEY, artifact_id TEXT NOT NULL, first_seen_at INTEGER NOT NULL)`,
  );
}

async function resetD1(): Promise<void> {
  await env.META.exec('DELETE FROM artifacts');
  await env.META.exec('DELETE FROM content_hashes');
  await env.META.exec('DELETE FROM authors');
}

interface Seed {
  id: string;
  authorPub: Uint8Array;
  name: string;
  kind: 'theme' | 'bundle';
  tags: string[];
  installs: number;
  createdAt: number;
}

async function seedAuthor(pub: Uint8Array, createdAt: number): Promise<void> {
  await env.META.prepare('INSERT OR IGNORE INTO authors (pubkey, created_at) VALUES (?, ?)')
    .bind(pub, createdAt)
    .run();
}

async function seed(rows: Seed[]): Promise<void> {
  for (const r of rows) {
    await seedAuthor(r.authorPub, r.createdAt);
    await env.META.prepare(
      `INSERT INTO artifacts (
         id, author_pubkey, name, kind, content_hash, thumbnail_hash,
         description, tags, license, version, omni_min_version, signature,
         created_at, updated_at, install_count
       ) VALUES (?, ?, ?, ?, ?, ?, '', ?, 'MIT', '1.0.0', '0.1.0',
                 X'00', ?, ?, ?)`,
    )
      .bind(
        r.id,
        r.authorPub,
        r.name,
        r.kind,
        `hash_${r.id}`,
        `thumb_${r.id}`,
        JSON.stringify(r.tags),
        r.createdAt,
        r.createdAt,
        r.installs,
      )
      .run();
  }
}

const AUTHOR_A = new Uint8Array(32).fill(0xaa);
const AUTHOR_B = new Uint8Array(32).fill(0xbb);

const DATASET: Seed[] = [
  // Ordered by created_at ASC for readability.
  {
    id: 'a1',
    authorPub: AUTHOR_A,
    name: 'Alpha',
    kind: 'theme',
    tags: ['dark'],
    installs: 10,
    createdAt: 1000,
  },
  {
    id: 'a2',
    authorPub: AUTHOR_A,
    name: 'Bravo',
    kind: 'bundle',
    tags: ['dark', 'gaming'],
    installs: 50,
    createdAt: 1100,
  },
  {
    id: 'a3',
    authorPub: AUTHOR_B,
    name: 'Charlie',
    kind: 'theme',
    tags: ['light'],
    installs: 30,
    createdAt: 1200,
  },
  {
    id: 'a4',
    authorPub: AUTHOR_B,
    name: 'Delta',
    kind: 'bundle',
    tags: ['gaming'],
    installs: 5,
    createdAt: 1300,
  },
  {
    id: 'a5',
    authorPub: AUTHOR_A,
    name: 'Echo',
    kind: 'theme',
    tags: ['minimal'],
    installs: 50,
    createdAt: 1400,
  },
];

interface ListBody {
  items: Array<{
    artifact_id: string;
    name: string;
    installs: number;
    tags: string[];
    kind: string;
  }>;
  next_cursor?: string;
}

async function getList(query: string): Promise<{ status: number; body: ListBody; res: Response }> {
  const res = await SELF.fetch(`https://worker.test/v1/list${query ? `?${query}` : ''}`);
  const body = (await res.json()) as ListBody;
  return { status: res.status, body, res };
}

beforeAll(async () => {
  await ensureSchema();
});

beforeEach(async () => {
  await resetD1();
  await seed(DATASET);
});

describe('GET /v1/list — sort=new', () => {
  it('returns created_at DESC with id ASC tie-break', async () => {
    const { status, body } = await getList('sort=new');
    expect(status).toBe(200);
    expect(body.items.map((i) => i.artifact_id)).toEqual(['a5', 'a4', 'a3', 'a2', 'a1']);
  });
});

describe('GET /v1/list — sort=installs', () => {
  it('returns install_count DESC with id ASC tie-break', async () => {
    const { status, body } = await getList('sort=installs');
    expect(status).toBe(200);
    // a2=50 and a5=50 tied → id ASC: a2, a5. Then a3=30, a1=10, a4=5.
    expect(body.items.map((i) => i.artifact_id)).toEqual(['a2', 'a5', 'a3', 'a1', 'a4']);
  });
});

describe('GET /v1/list — sort=name', () => {
  it('returns name ASC with id ASC tie-break', async () => {
    const { status, body } = await getList('sort=name');
    expect(status).toBe(200);
    expect(body.items.map((i) => i.name)).toEqual(['Alpha', 'Bravo', 'Charlie', 'Delta', 'Echo']);
  });
});

describe('GET /v1/list — tag filter', () => {
  it('single tag filters to matches', async () => {
    const { status, body } = await getList('tag=dark&sort=name');
    expect(status).toBe(200);
    expect(body.items.map((i) => i.artifact_id).sort()).toEqual(['a1', 'a2']);
  });
  it('multiple tags AND together', async () => {
    const { status, body } = await getList('tag=dark&tag=gaming');
    expect(status).toBe(200);
    expect(body.items.map((i) => i.artifact_id)).toEqual(['a2']);
  });
  it('unknown tag returns empty', async () => {
    const { status, body } = await getList('tag=nonexistent');
    expect(status).toBe(200);
    expect(body.items).toEqual([]);
  });
});

describe('GET /v1/list — kind filter', () => {
  it('kind=theme returns only themes', async () => {
    const { body } = await getList('kind=theme&sort=name');
    expect(body.items.map((i) => i.kind)).toEqual(['theme', 'theme', 'theme']);
  });
  it('kind=all (default) returns everything', async () => {
    const { body } = await getList('kind=all');
    expect(body.items.length).toBe(DATASET.length);
  });
});

describe('GET /v1/list — pagination cursor round-trip', () => {
  it('limit=2 returns cursor; follow-up returns next two excluding cursor row', async () => {
    const first = await getList('sort=new&limit=2');
    expect(first.status).toBe(200);
    expect(first.body.items.map((i) => i.artifact_id)).toEqual(['a5', 'a4']);
    expect(first.body.next_cursor).toBeDefined();
    const cur = decodeCursor(first.body.next_cursor!);
    expect(cur.i).toBe('a4');

    const second = await getList(
      `sort=new&limit=2&cursor=${encodeURIComponent(first.body.next_cursor!)}`,
    );
    expect(second.status).toBe(200);
    expect(second.body.items.map((i) => i.artifact_id)).toEqual(['a3', 'a2']);

    const third = await getList(
      `sort=new&limit=2&cursor=${encodeURIComponent(second.body.next_cursor!)}`,
    );
    expect(third.body.items.map((i) => i.artifact_id)).toEqual(['a1']);
    expect(third.body.next_cursor).toBeUndefined();
  });

  it('installs sort paginates through tied install_counts deterministically', async () => {
    const first = await getList('sort=installs&limit=2');
    expect(first.body.items.map((i) => i.artifact_id)).toEqual(['a2', 'a5']);
    const second = await getList(
      `sort=installs&limit=2&cursor=${encodeURIComponent(first.body.next_cursor!)}`,
    );
    expect(second.body.items.map((i) => i.artifact_id)).toEqual(['a3', 'a1']);
  });
});

describe('GET /v1/list — limit clamp', () => {
  it('limit below 1 clamps to 1', async () => {
    const { body } = await getList('sort=new&limit=0');
    expect(body.items.length).toBe(1);
  });
  it('limit above 100 clamps to 100 (data<100 returns all)', async () => {
    const { body } = await getList('sort=new&limit=9999');
    expect(body.items.length).toBe(DATASET.length);
  });
  it('missing limit defaults to 25', async () => {
    const { body } = await getList('sort=new');
    expect(body.items.length).toBe(DATASET.length); // <25
  });
});

describe('GET /v1/list — Cache-Control header', () => {
  it('responses carry public, max-age=60', async () => {
    const { res } = await getList('sort=new');
    expect(res.headers.get('Cache-Control')).toBe('public, max-age=60');
  });
});

describe('GET /v1/list — is_removed filter', () => {
  it('hides tombstoned rows', async () => {
    await env.META.prepare('UPDATE artifacts SET is_removed = 1 WHERE id = ?').bind('a3').run();
    const { body } = await getList('sort=name');
    expect(body.items.map((i) => i.artifact_id)).not.toContain('a3');
    expect(body.items.length).toBe(DATASET.length - 1);
  });
});

describe('GET /v1/list — malformed cursor', () => {
  it('400 BAD_REQUEST on unparseable cursor', async () => {
    const res = await SELF.fetch('https://worker.test/v1/list?cursor=@@not-valid@@');
    expect(res.status).toBe(400);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('BAD_REQUEST');
  });
});
