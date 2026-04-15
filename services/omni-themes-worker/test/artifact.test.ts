/**
 * Tier B — Miniflare-backed integration tests for /v1/artifact/:id (W3T11).
 *
 * Covers spec #008 §8 / §9 / §10 + contract §4.4 / §4.5 / §4.6:
 *   - GET 404 on unknown id
 *   - GET public view does NOT include `reports`
 *   - GET with non-moderator JWS does NOT include `reports`
 *   - GET with moderator JWS includes `reports`
 *   - PATCH FORBIDDEN when JWS kid != author_pubkey
 *   - DELETE FORBIDDEN likewise
 *   - DELETE happy path → 204, row is marked is_removed=1
 *   - PATCH "unchanged" return when DO reports same canonical_hash
 *
 * PATCH/DELETE auth is author-only (contract §4.5/§4.6). PATCH exercises the
 * forward-to-DO path; the DO itself is a 501 stub until W3T9 lands, so we
 * only assert the pre-DO gates (auth/ownership/rate-limit) in this file.
 * Full end-to-end PATCH happy-path lives in the W4 integration suite.
 */
import { describe, it, expect, beforeAll, beforeEach } from "vitest";
import { env, SELF as RAW_SELF } from "cloudflare:test";

// Inject the global client-version headers (W4T14) into every Miniflare
// request unless the test already set them.
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
  await env.META.exec(
    `CREATE TABLE IF NOT EXISTS content_hashes (content_hash TEXT PRIMARY KEY, artifact_id TEXT NOT NULL, first_seen_at INTEGER NOT NULL)`,
  );
}

// ---------- helpers ----------

const B64URL =
  "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

function b64urlEncode(bytes: Uint8Array | string): string {
  const b = typeof bytes === "string" ? new TextEncoder().encode(bytes) : bytes;
  let s = "";
  let i = 0;
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
  let s = "";
  for (let i = 0; i < b.length; i++) s += b[i]!.toString(16).padStart(2, "0");
  return s;
}

async function sha256Hex(data: ArrayBuffer | Uint8Array): Promise<string> {
  const buf = data instanceof Uint8Array ? data : new Uint8Array(data);
  const d = await crypto.subtle.digest("SHA-256", buf);
  return hexEncode(new Uint8Array(d));
}

async function signJws(opts: {
  method: string;
  path: string;
  body?: Uint8Array;
  seed: Uint8Array;
  pubkey: Uint8Array;
  df: Uint8Array;
}): Promise<string> {
  const body = opts.body ?? new Uint8Array(0);
  const claims = {
    method: opts.method,
    path: opts.path,
    ts: Math.floor(Date.now() / 1000),
    body_sha256: await sha256Hex(body),
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

// ---------- fixtures ----------

const AUTHOR_SEED = new Uint8Array(32).fill(0x22);
let AUTHOR_PUB: Uint8Array;
const AUTHOR_DF = new Uint8Array(32).fill(0x33);

const OTHER_SEED = new Uint8Array(32).fill(0x44);
let OTHER_PUB: Uint8Array;
const OTHER_DF = new Uint8Array(32).fill(0x55);

const MOD_SEED = new Uint8Array(32).fill(0x66);
let MOD_PUB: Uint8Array;
const MOD_DF = new Uint8Array(32).fill(0x77);

async function resetD1(): Promise<void> {
  await env.META.exec("DELETE FROM artifacts");
  await env.META.exec("DELETE FROM content_hashes");
  await env.META.exec("DELETE FROM authors");
}

async function resetKv(): Promise<void> {
  for (const prefix of ["quota:", "df_pubkey_velocity:", "denylist:"]) {
    const l = await env.STATE.list({ prefix });
    for (const k of l.keys) await env.STATE.delete(k.name);
  }
}

async function seedArtifact(opts: {
  id: string;
  authorPub: Uint8Array;
  name?: string;
  tags?: string[];
  reports?: number;
}): Promise<void> {
  const name = opts.name ?? `name-${opts.id}`;
  const tags = JSON.stringify(opts.tags ?? []);
  await env.META.prepare(
    "INSERT OR IGNORE INTO authors (pubkey, created_at) VALUES (?, 1000)",
  ).bind(opts.authorPub).run();
  await env.META.prepare(
    `INSERT INTO artifacts (
       id, author_pubkey, name, kind, content_hash, thumbnail_hash,
       description, tags, license, version, omni_min_version, signature,
       created_at, updated_at, install_count, report_count
     ) VALUES (?, ?, ?, 'theme', ?, ?, '', ?, 'MIT', '1.0.0', '0.1.0',
               X'00', 1000, 1000, 0, ?)`,
  ).bind(
    opts.id,
    opts.authorPub,
    name,
    `hash_${opts.id}`,
    `thumb_${opts.id}`,
    tags,
    opts.reports ?? 7,
  ).run();
}

beforeAll(async () => {
  await ensureSchema();
});

beforeEach(async () => {
  await resetD1();
  await resetKv();
  AUTHOR_PUB ||= await ed.getPublicKeyAsync(AUTHOR_SEED);
  OTHER_PUB ||= await ed.getPublicKeyAsync(OTHER_SEED);
  MOD_PUB ||= await ed.getPublicKeyAsync(MOD_SEED);
});

// ---------- GET /v1/artifact/:id ----------

describe("GET /v1/artifact/:id — 404", () => {
  it("unknown id returns NOT_FOUND", async () => {
    const res = await SELF.fetch("https://worker.test/v1/artifact/missing");
    expect(res.status).toBe(404);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe("NOT_FOUND");
  });
});

describe("GET /v1/artifact/:id — public view", () => {
  it("unauthenticated GET omits reports field", async () => {
    await seedArtifact({ id: "art1", authorPub: AUTHOR_PUB, reports: 7 });
    const res = await SELF.fetch("https://worker.test/v1/artifact/art1");
    expect(res.status).toBe(200);
    const body = (await res.json()) as Record<string, unknown>;
    expect(body.artifact_id).toBe("art1");
    expect(body.author_pubkey).toBe(hexEncode(AUTHOR_PUB));
    expect(body.status).toBe("live");
    expect("reports" in body).toBe(false);
  });
});

describe("GET /v1/artifact/:id — authed non-moderator", () => {
  it("still omits reports", async () => {
    await seedArtifact({ id: "art1", authorPub: AUTHOR_PUB, reports: 7 });
    const jws = await signJws({
      method: "GET", path: "/v1/artifact/art1",
      seed: AUTHOR_SEED, pubkey: AUTHOR_PUB, df: AUTHOR_DF,
    });
    const res = await SELF.fetch("https://worker.test/v1/artifact/art1", {
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(200);
    const body = (await res.json()) as Record<string, unknown>;
    expect("reports" in body).toBe(false);
  });
});

describe("GET /v1/artifact/:id — moderator JWS", () => {
  it("includes reports field when kid is on moderator allowlist", async () => {
    await seedArtifact({ id: "art1", authorPub: AUTHOR_PUB, reports: 7 });
    const modPubHex = hexEncode(MOD_PUB);
    const jws = await signJws({
      method: "GET", path: "/v1/artifact/art1",
      seed: MOD_SEED, pubkey: MOD_PUB, df: MOD_DF,
    });
    // Use app.request() rather than SELF.fetch so we can pass an env override —
    // Miniflare's SELF request pipeline snapshots env before the test mutation
    // reaches it. app.request runs the Hono app in-process using the env arg
    // we pass.
    const app = (await import("../src/index")).default;
    const modEnv = { ...env, OMNI_ADMIN_PUBKEYS: modPubHex };
    const res = await app.request(
      "/v1/artifact/art1",
      {
        headers: {
          Authorization: `Omni-JWS ${jws}`,
          "X-Omni-Version": "0.1.0",
          "X-Omni-Sanitize-Version": "1",
        },
      },
      modEnv,
    );
    expect(res.status).toBe(200);
    const body = (await res.json()) as Record<string, unknown>;
    expect(body.reports).toBe(7);
  });
});

describe("GET /v1/artifact/:id — tombstoned status", () => {
  it("status=tombstoned when is_removed=1", async () => {
    await seedArtifact({ id: "art1", authorPub: AUTHOR_PUB });
    await env.META.prepare("UPDATE artifacts SET is_removed=1 WHERE id=?")
      .bind("art1").run();
    const res = await SELF.fetch("https://worker.test/v1/artifact/art1");
    const body = (await res.json()) as { status: string };
    expect(body.status).toBe("tombstoned");
  });
});

// ---------- PATCH /v1/artifact/:id ----------

describe("PATCH /v1/artifact/:id — FORBIDDEN for non-author", () => {
  it("403 when JWS kid is not the author_pubkey", async () => {
    await seedArtifact({ id: "art1", authorPub: AUTHOR_PUB });
    const jws = await signJws({
      method: "PATCH", path: "/v1/artifact/art1",
      seed: OTHER_SEED, pubkey: OTHER_PUB, df: OTHER_DF,
    });
    const res = await SELF.fetch("https://worker.test/v1/artifact/art1", {
      method: "PATCH",
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(403);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe("FORBIDDEN");
  });
});

describe("PATCH /v1/artifact/:id — missing auth", () => {
  it("401 AUTH_MALFORMED_ENVELOPE", async () => {
    await seedArtifact({ id: "art1", authorPub: AUTHOR_PUB });
    const res = await SELF.fetch("https://worker.test/v1/artifact/art1", {
      method: "PATCH",
    });
    expect(res.status).toBe(401);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe("AUTH_MALFORMED_ENVELOPE");
  });
});

describe("PATCH /v1/artifact/:id — 404", () => {
  it("404 on unknown id even after auth passes", async () => {
    const jws = await signJws({
      method: "PATCH", path: "/v1/artifact/missing",
      seed: AUTHOR_SEED, pubkey: AUTHOR_PUB, df: AUTHOR_DF,
    });
    const res = await SELF.fetch("https://worker.test/v1/artifact/missing", {
      method: "PATCH",
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(404);
  });
});

// ---------- DELETE /v1/artifact/:id ----------

describe("DELETE /v1/artifact/:id — 204 on owner", () => {
  it("marks is_removed=1 and returns 204", async () => {
    await seedArtifact({ id: "art1", authorPub: AUTHOR_PUB });
    const jws = await signJws({
      method: "DELETE", path: "/v1/artifact/art1",
      seed: AUTHOR_SEED, pubkey: AUTHOR_PUB, df: AUTHOR_DF,
    });
    const res = await SELF.fetch("https://worker.test/v1/artifact/art1", {
      method: "DELETE",
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(204);
    const row = await env.META.prepare(
      "SELECT is_removed FROM artifacts WHERE id = ?",
    ).bind("art1").first<{ is_removed: number }>();
    expect(row?.is_removed).toBe(1);
  });
});

describe("DELETE /v1/artifact/:id — FORBIDDEN for non-author", () => {
  it("403 when JWS kid is not the author_pubkey", async () => {
    await seedArtifact({ id: "art1", authorPub: AUTHOR_PUB });
    const jws = await signJws({
      method: "DELETE", path: "/v1/artifact/art1",
      seed: OTHER_SEED, pubkey: OTHER_PUB, df: OTHER_DF,
    });
    const res = await SELF.fetch("https://worker.test/v1/artifact/art1", {
      method: "DELETE",
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(403);
    const j = (await res.json()) as { error: { code: string } };
    expect(j.error.code).toBe("FORBIDDEN");
  });
});

describe("DELETE /v1/artifact/:id — 404", () => {
  it("404 on unknown id after auth passes", async () => {
    const jws = await signJws({
      method: "DELETE", path: "/v1/artifact/nope",
      seed: AUTHOR_SEED, pubkey: AUTHOR_PUB, df: AUTHOR_DF,
    });
    const res = await SELF.fetch("https://worker.test/v1/artifact/nope", {
      method: "DELETE",
      headers: { Authorization: `Omni-JWS ${jws}` },
    });
    expect(res.status).toBe(404);
  });
});
