/**
 * Tier B — Miniflare-backed integration tests for GET /v1/me/gallery (W3T11).
 *
 * Covers spec #008 §12, contract §4.8:
 *   - 401 when unauthenticated
 *   - Returns only artifacts whose author_pubkey matches the JWS kid
 *   - Excludes is_removed=1 rows
 *   - Orders by updated_at DESC
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
import * as ed from '@noble/ed25519';
import type { Env } from '../src/env';
import { signJws } from './helpers/signer';

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
}

async function signGet(opts: {
  path: string;
  seed: Uint8Array;
  pubkey: Uint8Array;
  df: Uint8Array;
}): Promise<string> {
  return signJws({
    method: 'GET',
    path: opts.path,
    seed: opts.seed,
    pubkey: opts.pubkey,
    df: opts.df,
  });
}

const A_SEED = new Uint8Array(32).fill(0x10);
const A_DF = new Uint8Array(32).fill(0x11);
const B_SEED = new Uint8Array(32).fill(0x20);
let A_PUB: Uint8Array;
let B_PUB: Uint8Array;

async function resetD1(): Promise<void> {
  await env.META.exec('DELETE FROM artifacts');
  await env.META.exec('DELETE FROM authors');
}
async function resetKv(): Promise<void> {
  for (const prefix of ['quota:', 'df_pubkey_velocity:', 'denylist:']) {
    const l = await env.STATE.list({ prefix });
    for (const k of l.keys) await env.STATE.delete(k.name);
  }
}

async function seedArtifact(opts: {
  id: string;
  authorPub: Uint8Array;
  name: string;
  updatedAt: number;
  isRemoved?: boolean;
}): Promise<void> {
  await env.META.prepare('INSERT OR IGNORE INTO authors (pubkey, created_at) VALUES (?, 1000)')
    .bind(opts.authorPub)
    .run();
  await env.META.prepare(
    `INSERT INTO artifacts (
       id, author_pubkey, name, kind, content_hash, thumbnail_hash,
       description, tags, license, version, omni_min_version, signature,
       created_at, updated_at, install_count, is_removed
     ) VALUES (?, ?, ?, 'theme', ?, ?, '', '[]', 'MIT', '1.0.0', '0.1.0',
               X'00', 1000, ?, 0, ?)`,
  )
    .bind(
      opts.id,
      opts.authorPub,
      opts.name,
      `hash_${opts.id}`,
      `thumb_${opts.id}`,
      opts.updatedAt,
      opts.isRemoved ? 1 : 0,
    )
    .run();
}

beforeAll(async () => {
  await ensureSchema();
});

beforeEach(async () => {
  await resetD1();
  await resetKv();
  A_PUB ||= await ed.getPublicKeyAsync(A_SEED);
  B_PUB ||= await ed.getPublicKeyAsync(B_SEED);
});

describe('GET /v1/me/gallery — requires auth', () => {
  it('401 when Authorization header is missing', async () => {
    const res = await SELF.fetch('https://worker.test/v1/me/gallery');
    expect(res.status).toBe(401);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe('AUTH_MALFORMED_ENVELOPE');
  });
});

describe('GET /v1/me/gallery — scope to author', () => {
  it('returns only artifacts authored by the calling pubkey', async () => {
    await seedArtifact({ id: 'a1', authorPub: A_PUB, name: 'A-one', updatedAt: 2000 });
    await seedArtifact({ id: 'a2', authorPub: A_PUB, name: 'A-two', updatedAt: 3000 });
    await seedArtifact({ id: 'b1', authorPub: B_PUB, name: 'B-one', updatedAt: 4000 });

    const jws = await signGet({
      path: '/v1/me/gallery',
      seed: A_SEED,
      pubkey: A_PUB,
      df: A_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/me/gallery', {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      items: Array<{ artifact_id: string; name: string; updated_at: number }>;
    };
    expect(body.items.map((i) => i.artifact_id).sort()).toEqual(['a1', 'a2']);
  });
});

describe('GET /v1/me/gallery — excludes tombstoned', () => {
  it('is_removed=1 rows are hidden even if authored by caller', async () => {
    await seedArtifact({ id: 'live', authorPub: A_PUB, name: 'live', updatedAt: 2000 });
    await seedArtifact({
      id: 'gone',
      authorPub: A_PUB,
      name: 'gone',
      updatedAt: 3000,
      isRemoved: true,
    });

    const jws = await signGet({
      path: '/v1/me/gallery',
      seed: A_SEED,
      pubkey: A_PUB,
      df: A_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/me/gallery', {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    const body = (await res.json()) as { items: Array<{ artifact_id: string }> };
    expect(body.items.map((i) => i.artifact_id)).toEqual(['live']);
  });
});

describe('GET /v1/me/gallery — author_display_name JOIN (spec §4.4 / OWI-79)', () => {
  // Per identity-completion-and-display-name spec §4.4: gallery responses embed
  // `author_display_name` via LEFT JOIN authors so the caller's own gallery
  // grid renders without N+1 lookups.
  it('includes author_display_name when caller has set one', async () => {
    await env.META.prepare(
      `INSERT OR REPLACE INTO authors (pubkey, display_name, created_at)
       VALUES (?, ?, ?)`,
    )
      .bind(A_PUB, 'starfire', 1000)
      .run();
    await seedArtifact({ id: 'g1', authorPub: A_PUB, name: 'gallery-one', updatedAt: 2000 });

    const jws = await signGet({
      path: '/v1/me/gallery',
      seed: A_SEED,
      pubkey: A_PUB,
      df: A_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/me/gallery', {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      items: Array<{ artifact_id: string; author_display_name: string | null }>;
    };
    expect(body.items.length).toBe(1);
    expect(body.items[0]!.author_display_name).toBe('starfire');
  });

  it('returns null author_display_name when authors row has display_name = NULL', async () => {
    // seedArtifact's INSERT OR IGNORE leaves display_name NULL by default.
    await seedArtifact({ id: 'g2', authorPub: A_PUB, name: 'gallery-two', updatedAt: 3000 });

    const jws = await signGet({
      path: '/v1/me/gallery',
      seed: A_SEED,
      pubkey: A_PUB,
      df: A_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/me/gallery', {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      items: Array<{ artifact_id: string; author_display_name: string | null }>;
    };
    expect(body.items.length).toBe(1);
    expect(body.items[0]!.author_display_name).toBeNull();
  });
});

describe('GET /v1/me/gallery — ordering', () => {
  it('orders by updated_at DESC', async () => {
    await seedArtifact({ id: 'older', authorPub: A_PUB, name: 'older', updatedAt: 1000 });
    await seedArtifact({ id: 'newer', authorPub: A_PUB, name: 'newer', updatedAt: 9999 });
    await seedArtifact({ id: 'mid', authorPub: A_PUB, name: 'mid', updatedAt: 5000 });

    const jws = await signGet({
      path: '/v1/me/gallery',
      seed: A_SEED,
      pubkey: A_PUB,
      df: A_DF,
    });
    const res = await SELF.fetch('https://worker.test/v1/me/gallery', {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    const body = (await res.json()) as { items: Array<{ artifact_id: string }> };
    expect(body.items.map((i) => i.artifact_id)).toEqual(['newer', 'mid', 'older']);
  });
});
