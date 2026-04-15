/**
 * Tier B — Miniflare-backed integration tests for GET /v1/me/gallery (W3T11).
 *
 * Covers spec #008 §12, contract §4.8:
 *   - 401 when unauthenticated
 *   - Returns only artifacts whose author_pubkey matches the JWS kid
 *   - Excludes is_removed=1 rows
 *   - Orders by updated_at DESC
 */
import { describe, it, expect, beforeAll, beforeEach } from "vitest";
import { env, SELF as RAW_SELF } from "cloudflare:test";

// Inject the W4T14 client-version headers into every Miniflare request.
const SELF = {
  fetch(input: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    if (!headers.has("X-Omni-Version")) headers.set("X-Omni-Version", "0.1.0");
    if (!headers.has("X-Omni-Sanitize-Version")) headers.set("X-Omni-Sanitize-Version", "1");
    return RAW_SELF.fetch(input, { ...init, headers });
  },
};
import * as ed from "@noble/ed25519";
import type { Env } from "../src/env";

declare module "cloudflare:test" {
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

const B64URL =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
function b64urlEncode(bytes: Uint8Array | string): string {
  const b = typeof bytes === "string" ? new TextEncoder().encode(bytes) : bytes;
  let s = ""; let i = 0;
  for (; i + 3 <= b.length; i += 3) {
    const n = (b[i]! << 16) | (b[i + 1]! << 8) | b[i + 2]!;
    s += B64URL[(n >> 18) & 63] + B64URL[(n >> 12) & 63] +
         B64URL[(n >> 6) & 63] + B64URL[n & 63];
  }
  if (i < b.length) {
    const rem = b.length - i;
    const n = (b[i]! << 16) | ((rem > 1 ? b[i + 1]! : 0) << 8);
    s += B64URL[(n >> 18) & 63] + B64URL[(n >> 12) & 63];
    if (rem === 2) s += B64URL[(n >> 6) & 63];
  }
  return s;
}
function hexEncode(b: Uint8Array): string {
  let s = ""; for (let i = 0; i < b.length; i++) s += b[i]!.toString(16).padStart(2, "0"); return s;
}
async function sha256Hex(data: Uint8Array): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", data);
  return hexEncode(new Uint8Array(d));
}
async function signGet(opts: {
  path: string; seed: Uint8Array; pubkey: Uint8Array; df: Uint8Array;
}): Promise<string> {
  const claims = {
    method: "GET",
    path: opts.path,
    ts: Math.floor(Date.now() / 1000),
    body_sha256: await sha256Hex(new Uint8Array(0)),
    query_sha256: await sha256Hex(new TextEncoder().encode("")),
    sanitize_version: 1,
    kid: hexEncode(opts.pubkey),
    df: hexEncode(opts.df),
  };
  const headerB64 = b64urlEncode('{"typ":"JWT","alg":"EdDSA"}');
  const payloadB64 = b64urlEncode(JSON.stringify(claims));
  const signingInput = new TextEncoder().encode(`${headerB64}.${payloadB64}`);
  const sig = await ed.signAsync(signingInput, opts.seed);
  return `${headerB64}.${payloadB64}.${b64urlEncode(sig)}`;
}

const A_SEED = new Uint8Array(32).fill(0x10);
const A_DF = new Uint8Array(32).fill(0x11);
const B_SEED = new Uint8Array(32).fill(0x20);
let A_PUB: Uint8Array;
let B_PUB: Uint8Array;

async function resetD1(): Promise<void> {
  await env.META.exec("DELETE FROM artifacts");
  await env.META.exec("DELETE FROM authors");
}
async function resetKv(): Promise<void> {
  for (const prefix of ["quota:", "df_pubkey_velocity:", "denylist:"]) {
    const l = await env.STATE.list({ prefix });
    for (const k of l.keys) await env.STATE.delete(k.name);
  }
}

async function seedArtifact(opts: {
  id: string; authorPub: Uint8Array; name: string; updatedAt: number; isRemoved?: boolean;
}): Promise<void> {
  await env.META.prepare(
    "INSERT OR IGNORE INTO authors (pubkey, created_at) VALUES (?, 1000)",
  ).bind(opts.authorPub).run();
  await env.META.prepare(
    `INSERT INTO artifacts (
       id, author_pubkey, name, kind, content_hash, thumbnail_hash,
       description, tags, license, version, omni_min_version, signature,
       created_at, updated_at, install_count, is_removed
     ) VALUES (?, ?, ?, 'theme', ?, ?, '', '[]', 'MIT', '1.0.0', '0.1.0',
               X'00', 1000, ?, 0, ?)`,
  ).bind(
    opts.id, opts.authorPub, opts.name,
    `hash_${opts.id}`, `thumb_${opts.id}`,
    opts.updatedAt, opts.isRemoved ? 1 : 0,
  ).run();
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

describe("GET /v1/me/gallery — requires auth", () => {
  it("401 when Authorization header is missing", async () => {
    const res = await SELF.fetch("https://worker.test/v1/me/gallery");
    expect(res.status).toBe(401);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe("AUTH_MALFORMED_ENVELOPE");
  });
});

describe("GET /v1/me/gallery — scope to author", () => {
  it("returns only artifacts authored by the calling pubkey", async () => {
    await seedArtifact({ id: "a1", authorPub: A_PUB, name: "A-one",   updatedAt: 2000 });
    await seedArtifact({ id: "a2", authorPub: A_PUB, name: "A-two",   updatedAt: 3000 });
    await seedArtifact({ id: "b1", authorPub: B_PUB, name: "B-one",   updatedAt: 4000 });

    const jws = await signGet({
      path: "/v1/me/gallery", seed: A_SEED, pubkey: A_PUB, df: A_DF,
    });
    const res = await SELF.fetch("https://worker.test/v1/me/gallery", {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as {
      items: Array<{ artifact_id: string; name: string; updated_at: number }>;
    };
    expect(body.items.map((i) => i.artifact_id).sort()).toEqual(["a1", "a2"]);
  });
});

describe("GET /v1/me/gallery — excludes tombstoned", () => {
  it("is_removed=1 rows are hidden even if authored by caller", async () => {
    await seedArtifact({ id: "live", authorPub: A_PUB, name: "live",   updatedAt: 2000 });
    await seedArtifact({ id: "gone", authorPub: A_PUB, name: "gone",   updatedAt: 3000, isRemoved: true });

    const jws = await signGet({
      path: "/v1/me/gallery", seed: A_SEED, pubkey: A_PUB, df: A_DF,
    });
    const res = await SELF.fetch("https://worker.test/v1/me/gallery", {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    const body = (await res.json()) as { items: Array<{ artifact_id: string }> };
    expect(body.items.map((i) => i.artifact_id)).toEqual(["live"]);
  });
});

describe("GET /v1/me/gallery — ordering", () => {
  it("orders by updated_at DESC", async () => {
    await seedArtifact({ id: "older", authorPub: A_PUB, name: "older", updatedAt: 1000 });
    await seedArtifact({ id: "newer", authorPub: A_PUB, name: "newer", updatedAt: 9999 });
    await seedArtifact({ id: "mid",   authorPub: A_PUB, name: "mid",   updatedAt: 5000 });

    const jws = await signGet({
      path: "/v1/me/gallery", seed: A_SEED, pubkey: A_PUB, df: A_DF,
    });
    const res = await SELF.fetch("https://worker.test/v1/me/gallery", {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    const body = (await res.json()) as { items: Array<{ artifact_id: string }> };
    expect(body.items.map((i) => i.artifact_id)).toEqual(["newer", "mid", "older"]);
  });
});
