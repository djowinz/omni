import { describe, it, expect, beforeAll, beforeEach } from "vitest";
import { env, applyD1Migrations } from "cloudflare:test";
import type { Env } from "../src/env";
import app from "../src/index";
import { signJws as signJwsShared } from "./helpers/signer";

/**
 * Tier B — Miniflare-backed tests for POST /v1/report (plan #008 W3T13,
 * contract §4.7). The test mints compact JWS envelopes byte-for-byte
 * equivalent to the native oracle (see `crates/omni-identity/src/
 * wasm_jws_core.rs`), hits the real route through `app.request`, and
 * inspects KV (`reports:<uuid>`) + D1 (`artifacts.report_count`) for
 * durable side-effects.
 *
 * A minimal D1 schema is seeded via `applyD1Migrations` using the same
 * `migrations/0001_initial_schema.sql` file wrangler applies in prod.
 */

declare module "cloudflare:test" {
  interface ProvidedEnv extends Env {}
}

// ---------------------------------------------------------------------------
// Fixture key material (plan #008 W1T3 — seed 0x07 * 32, matching native).
// ---------------------------------------------------------------------------
const SEED_HEX =
  "0707070707070707070707070707070707070707070707070707070707070707";
const PUBKEY_HEX =
  "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";
const DF_HEX =
  "dc9773ca5d79ecfdedf0c8cca1cfecac9bc39c09550aec75a8cbe8b2a13b67a1";

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++)
    out[i] = parseInt(hex.substr(i * 2, 2), 16);
  return out;
}

const SEED = hexToBytes(SEED_HEX);

/** Make a fresh, valid 64-char hex DF per-test so daily quotas don't bleed. */
let dfCounter = 0;
function freshDfHex(): string {
  dfCounter += 1;
  const suffix = dfCounter.toString(16).padStart(8, "0");
  // 64 hex chars — 32 bytes. Prefix with zeros, tail with counter.
  return ("00".repeat(32)).slice(0, 56) + suffix;
}

interface SignOptions {
  method?: string;
  path?: string;
  query?: string;
  body?: Uint8Array;
  ts?: number;
  sanitizeVersion?: number;
  kidHex?: string;
  dfHex?: string;
}

async function signJws(o: SignOptions = {}): Promise<string> {
  const kid = o.kidHex ?? PUBKEY_HEX;
  const df = o.dfHex ?? DF_HEX;
  return signJwsShared({
    method: o.method ?? "POST",
    path: o.path ?? "/v1/report",
    body: o.body,
    query: o.query,
    seed: SEED,
    pubkey: hexToBytes(kid),
    df: hexToBytes(df),
    ts: o.ts,
    sanitizeVersion: o.sanitizeVersion,
  });
}

/** Build a `Request` the Hono app can dispatch through its `/v1/report` mount. */
async function mkRequest(
  bodyObj: unknown,
  opts: {
    dfHex?: string;
    kidHex?: string;
    includeAuth?: boolean;
    rawBody?: Uint8Array; // override the body bytes (for malformed-JSON tests)
  } = {},
): Promise<Request> {
  const bodyBytes =
    opts.rawBody ?? new TextEncoder().encode(JSON.stringify(bodyObj));
  const headers = new Headers({
    "content-type": "application/json",
    "X-Omni-Version": "0.1.0",
    "X-Omni-Sanitize-Version": "1",
  });
  if (opts.includeAuth !== false) {
    const jws = await signJws({
      body: bodyBytes,
      path: "/v1/report",
      method: "POST",
      kidHex: opts.kidHex,
      dfHex: opts.dfHex,
    });
    headers.set("Authorization", `Omni-JWS ${jws}`);
  }
  // Request requires an ArrayBuffer view; slice to a standalone buffer so
  // workerd doesn't complain about SharedArrayBuffer-typed backings.
  const buf = bodyBytes.buffer.slice(
    bodyBytes.byteOffset,
    bodyBytes.byteOffset + bodyBytes.byteLength,
  ) as ArrayBuffer;
  return new Request("https://worker.test/v1/report", {
    method: "POST",
    headers,
    body: buf,
  });
}

// ---------------------------------------------------------------------------
// D1 schema seeding. The same migration file wrangler applies in prod.
// ---------------------------------------------------------------------------
const ARTIFACT_ID = "artifact_under_test";
const OTHER_PUBKEY_BYTES = new Uint8Array(32).fill(0xaa);

async function seedArtifact(id: string = ARTIFACT_ID): Promise<void> {
  // Author row (FK target).
  await env.META.prepare(
    `INSERT OR IGNORE INTO authors (pubkey, display_name, created_at)
     VALUES (?, ?, ?)`,
  )
    .bind(OTHER_PUBKEY_BYTES, "seed_author", Math.floor(Date.now() / 1000))
    .run();
  await env.META.prepare(
    `INSERT OR IGNORE INTO artifacts (
       id, author_pubkey, name, kind, content_hash, thumbnail_hash,
       version, omni_min_version, signature, created_at, updated_at
     ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
  )
    .bind(
      id,
      OTHER_PUBKEY_BYTES,
      `name_${id}`,
      "theme",
      `hash_${id}`,
      `thumb_${id}`,
      "1.0.0",
      "0.1.0",
      new Uint8Array(64),
      Math.floor(Date.now() / 1000),
      Math.floor(Date.now() / 1000),
    )
    .run();
}

beforeAll(async () => {
  // Apply the committed migration file to the miniflare D1 binding. The
  // vitest-pool-workers runtime injects migrations via the test config when
  // `migrations_dir` is set in wrangler.toml; applyD1Migrations is a no-op
  // if they're already present (the d1_migrations table is idempotent).
  const migrations = await import("cloudflare:test").then(
    (m) =>
      (m as unknown as { listMigrations?: () => Promise<unknown> })
        .listMigrations?.(),
  );
  if (migrations) {
    await applyD1Migrations(
      env.META,
      migrations as unknown as Parameters<typeof applyD1Migrations>[1],
    );
  } else {
    // Fallback: inline the schema we rely on (artifacts + authors).
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS authors (
         pubkey BLOB PRIMARY KEY,
         display_name TEXT UNIQUE,
         created_at INTEGER NOT NULL,
         total_uploads INTEGER NOT NULL DEFAULT 0,
         is_new_creator INTEGER NOT NULL DEFAULT 1,
         is_denied INTEGER NOT NULL DEFAULT 0
       )`.replace(/\s+/g, " "),
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS artifacts (
         id TEXT PRIMARY KEY,
         author_pubkey BLOB NOT NULL REFERENCES authors(pubkey),
         name TEXT NOT NULL,
         kind TEXT NOT NULL,
         content_hash TEXT NOT NULL,
         thumbnail_hash TEXT NOT NULL,
         description TEXT,
         tags TEXT,
         license TEXT,
         version TEXT NOT NULL,
         omni_min_version TEXT NOT NULL,
         signature BLOB NOT NULL,
         created_at INTEGER NOT NULL,
         updated_at INTEGER NOT NULL,
         install_count INTEGER NOT NULL DEFAULT 0,
         report_count INTEGER NOT NULL DEFAULT 0,
         is_removed INTEGER NOT NULL DEFAULT 0,
         is_featured INTEGER NOT NULL DEFAULT 0,
         UNIQUE (author_pubkey, name)
       )`.replace(/\s+/g, " "),
    );
  }
});

beforeEach(async () => {
  // Fresh artifact row per test so report_count assertions are deterministic;
  // DELETE cascades aren't wired so we just overwrite the row.
  await env.META.prepare(`DELETE FROM artifacts WHERE id = ?`)
    .bind(ARTIFACT_ID)
    .run();
  await seedArtifact(ARTIFACT_ID);
  // Clear the velocity + quota KVs for our fixture DF so tests don't
  // interact (report quota is daily — use a fresh DF per-test where needed).
  await env.STATE.delete(`df_pubkey_velocity:${DF_HEX}`);
});

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

describe("POST /v1/report — happy path", () => {
  it("accepts a valid report, persists KV, and bumps report_count", async () => {
    const df = freshDfHex();
    const req = await mkRequest(
      {
        artifact_id: ARTIFACT_ID,
        category: "malware",
        note: "suspicious behavior",
      },
      { dfHex: df },
    );
    const res = await app.fetch(req, env);
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as { report_id: string; status: string };
    expect(body.status).toBe("received");
    expect(body.report_id).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i,
    );

    // KV side-effect.
    const stored = await env.STATE.get(`reports:${body.report_id}`);
    expect(stored).not.toBeNull();
    const rec = JSON.parse(stored!);
    expect(rec.artifact_id).toBe(ARTIFACT_ID);
    expect(rec.category).toBe("malware");
    expect(rec.note).toBe("suspicious behavior");
    expect(rec.reporter_pubkey).toBe(PUBKEY_HEX);
    expect(rec.reporter_df).toBe(df);
    expect(typeof rec.received_at).toBe("number");

    // D1 side-effect.
    const row = await env.META.prepare(
      "SELECT report_count FROM artifacts WHERE id = ?",
    )
      .bind(ARTIFACT_ID)
      .first<{ report_count: number }>();
    expect(row?.report_count).toBe(1);
  });

  it("accepts a valid report with omitted note (note is optional)", async () => {
    const df = freshDfHex();
    const req = await mkRequest(
      { artifact_id: ARTIFACT_ID, category: "nsfw" },
      { dfHex: df },
    );
    const res = await app.fetch(req, env);
    expect(res.status).toBe(200);
    const body = (await res.json()) as { report_id: string };
    const stored = await env.STATE.get(`reports:${body.report_id}`);
    expect(stored).not.toBeNull();
    const rec = JSON.parse(stored!);
    expect(rec.note).toBeNull();
  });
});

describe("POST /v1/report — authentication", () => {
  it("missing Authorization → 401 AUTH_MALFORMED_ENVELOPE", async () => {
    const req = await mkRequest(
      { artifact_id: ARTIFACT_ID, category: "other" },
      { includeAuth: false },
    );
    const res = await app.fetch(req, env);
    expect(res.status).toBe(401);
    const body = (await res.json()) as {
      error: { code: string };
      kind?: string;
    };
    expect(body.error.code).toBe("AUTH_MALFORMED_ENVELOPE");
    expect(body.kind).toBe("Auth");
  });
});

describe("POST /v1/report — body validation", () => {
  it("unknown category → 400 BAD_REQUEST kind=Malformed", async () => {
    const df = freshDfHex();
    const req = await mkRequest(
      { artifact_id: ARTIFACT_ID, category: "spam" /* not in set */ },
      { dfHex: df },
    );
    const res = await app.fetch(req, env);
    expect(res.status).toBe(400);
    const body = (await res.json()) as {
      error: { code: string };
      kind?: string;
      detail?: string;
    };
    expect(body.error.code).toBe("BAD_REQUEST");
    expect(body.kind).toBe("Malformed");
  });

  it("missing artifact_id → 400 BAD_REQUEST", async () => {
    const df = freshDfHex();
    const req = await mkRequest({ category: "malware" }, { dfHex: df });
    const res = await app.fetch(req, env);
    expect(res.status).toBe(400);
    const body = (await res.json()) as {
      error: { code: string };
      kind?: string;
    };
    expect(body.error.code).toBe("BAD_REQUEST");
    expect(body.kind).toBe("Malformed");
  });

  it("note > 500 chars → 400 BAD_REQUEST kind=Malformed", async () => {
    const df = freshDfHex();
    const req = await mkRequest(
      {
        artifact_id: ARTIFACT_ID,
        category: "other",
        note: "a".repeat(501),
      },
      { dfHex: df },
    );
    const res = await app.fetch(req, env);
    expect(res.status).toBe(400);
    const body = (await res.json()) as {
      error: { code: string; message: string };
      kind?: string;
    };
    expect(body.error.code).toBe("BAD_REQUEST");
    expect(body.kind).toBe("Malformed");
    expect(body.error.message.toLowerCase()).toContain("note");
  });

  it("note exactly 500 chars is accepted (boundary)", async () => {
    const df = freshDfHex();
    const req = await mkRequest(
      {
        artifact_id: ARTIFACT_ID,
        category: "impersonation",
        note: "b".repeat(500),
      },
      { dfHex: df },
    );
    const res = await app.fetch(req, env);
    expect(res.status).toBe(200);
  });
});

describe("POST /v1/report — missing artifact", () => {
  it("unknown artifact_id → 404 NOT_FOUND", async () => {
    const df = freshDfHex();
    const req = await mkRequest(
      { artifact_id: "does_not_exist_xyz", category: "illegal" },
      { dfHex: df },
    );
    const res = await app.fetch(req, env);
    expect(res.status).toBe(404);
    const body = (await res.json()) as {
      error: { code: string };
      kind?: string;
      detail?: string;
    };
    expect(body.error.code).toBe("NOT_FOUND");
    expect(body.kind).toBe("Malformed");
    expect(body.detail).toBe("NotFound");
  });
});

describe("POST /v1/report — rate limiting", () => {
  it("21st report from a single DF in a day → 429 RATE_LIMITED", async () => {
    // Fresh DF so the daily counter starts empty. Use a stable hex DF that
    // is 32 bytes; the dfHex claim must round-trip through hex decoder.
    const df = "ab".repeat(32); // 64 hex chars = 32 bytes
    // Clear any prior state just in case.
    await env.STATE.delete(`df_pubkey_velocity:${df}`);
    const now = new Date();
    const y = now.getUTCFullYear();
    const m = String(now.getUTCMonth() + 1).padStart(2, "0");
    const d = String(now.getUTCDate()).padStart(2, "0");
    await env.STATE.delete(`quota:device:${df}:${y}-${m}-${d}`);
    await env.STATE.delete(`quota:pubkey:${PUBKEY_HEX}:${y}-${m}-${d}`);

    // 20 allowed reports.
    for (let i = 0; i < 20; i++) {
      const req = await mkRequest(
        { artifact_id: ARTIFACT_ID, category: "other", note: `r${i}` },
        { dfHex: df },
      );
      const res = await app.fetch(req, env);
      expect(res.status, `call ${i + 1}/20`).toBe(200);
    }

    // 21st — quota exhausted.
    const req = await mkRequest(
      { artifact_id: ARTIFACT_ID, category: "other" },
      { dfHex: df },
    );
    const res = await app.fetch(req, env);
    expect(res.status).toBe(429);
    const body = (await res.json()) as {
      error: { code: string; retry_after?: number };
      kind?: string;
    };
    expect(body.error.code).toBe("RATE_LIMITED");
    expect(body.kind).toBe("Quota");
    expect(body.error.retry_after).toBeGreaterThan(0);
  });
});
