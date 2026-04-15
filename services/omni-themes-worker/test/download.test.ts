/**
 * Tier B — Miniflare-backed integration tests for GET /v1/download/:id (W3T10).
 *
 * Coverage:
 *   - 404 on missing artifact
 *   - 410 on tombstoned artifact
 *   - rate-limit (unauthed, IP-keyed) via quota pre-fill
 *   - happy path headers: X-Omni-Content-Hash, X-Omni-Author-Pubkey,
 *                         X-Omni-Signature, X-Omni-Manifest
 *   - install_count + install_daily are bumped
 *
 * Bundle bytes are minted inline via `loadWasm` (see do.test.ts rationale —
 * `node:fs` unavailable in workers runtime).
 */
import { describe, it, expect, beforeAll, beforeEach } from "vitest";
import { env, SELF, applyD1Migrations } from "cloudflare:test";
import type { Env } from "../src/env";
import { loadWasm } from "../src/lib/wasm";

declare module "cloudflare:test" {
  interface ProvidedEnv extends Env {}
}

const SEED_HEX = "0707070707070707070707070707070707070707070707070707070707070707";
const PUBKEY_HEX = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c";

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}
function bytesToHex(b: Uint8Array): string {
  let s = ""; for (let i = 0; i < b.length; i++) s += b[i]!.toString(16).padStart(2, "0"); return s;
}
async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", bytes);
  return bytesToHex(new Uint8Array(d));
}

const SEED = hexToBytes(SEED_HEX);

const OVERLAY_BYTES = new TextEncoder().encode(
  '<overlay><template><div data-sensor="cpu.usage"/></template></overlay>',
);
const THEME_CSS_BYTES = new TextEncoder().encode(
  "/* test */\nbody { background: #111; }\n",
);

async function buildThemeBundle(): Promise<{ bytes: Uint8Array; contentHash: string }> {
  const { identity, bundle } = await loadWasm();
  const entries = [
    { path: "overlay.omni", bytes: OVERLAY_BYTES },
    { path: "themes/default.css", bytes: THEME_CSS_BYTES },
  ];
  const manifest = {
    schema_version: 1,
    name: "download-test",
    version: "1.0.0",
    omni_min_version: "0.1.0",
    description: "inline fixture for download.test.ts",
    tags: [],
    license: "MIT",
    entry_overlay: "overlay.omni",
    default_theme: "themes/default.css",
    sensor_requirements: [],
    files: await Promise.all(
      entries.map(async (f) => ({ path: f.path, sha256: await sha256Hex(f.bytes) })),
    ),
    resource_kinds: {
      theme: { dir: "themes/", extensions: [".css"], max_size_bytes: 1_048_576 },
    },
  };
  const filesMap = new Map(entries.map((f) => [f.path, f.bytes] as const));
  const bytes = identity.packSignedBundle(manifest, filesMap, SEED, undefined);
  const hashBytes = bundle.canonicalHash(manifest) as Uint8Array;
  return { bytes, contentHash: bytesToHex(hashBytes) };
}

async function resetEnv() {
  const ql = await env.STATE.list({ prefix: "quota:" });
  for (const k of ql.keys) await env.STATE.delete(k.name);
  await env.META.exec("DELETE FROM artifacts");
  await env.META.exec("DELETE FROM content_hashes");
  await env.META.exec("DELETE FROM authors");
  await env.META.exec("DELETE FROM tombstones");
  await env.META.exec("DELETE FROM install_daily");
}

async function seedArtifact(opts: { isRemoved?: boolean } = {}): Promise<{
  id: string; contentHash: string;
}> {
  const { bytes, contentHash } = await buildThemeBundle();
  const id = crypto.randomUUID();
  const now = Math.floor(Date.now() / 1000);
  const authorBlob = hexToBytes(PUBKEY_HEX);
  const sigBlob = new Uint8Array(32).fill(1);
  const thumbHash = "00".repeat(32);
  await env.META.prepare(
    `INSERT INTO authors (pubkey, created_at) VALUES (?, ?)
     ON CONFLICT(pubkey) DO NOTHING`,
  ).bind(authorBlob, now).run();
  await env.META.prepare(
    `INSERT INTO artifacts (id, author_pubkey, name, kind, content_hash,
       thumbnail_hash, description, tags, license, version, omni_min_version,
       signature, created_at, updated_at, install_count, report_count,
       is_removed, is_featured)
     VALUES (?, ?, ?, 'theme', ?, ?, NULL, '[]', 'MIT', '1.0.0', '0.1.0',
             ?, ?, ?, 0, 0, ?, 0)`,
  ).bind(
    id, authorBlob, "download-test",
    contentHash, thumbHash, sigBlob, now, now, opts.isRemoved ? 1 : 0,
  ).run();
  await env.BLOBS.put(`bundles/${contentHash}.omnipkg`, bytes);
  return { id, contentHash };
}

beforeAll(async () => {
  const migrations = await import("cloudflare:test").then(
    (m) => (m as unknown as { listMigrations?: () => Promise<unknown> }).listMigrations?.(),
  );
  if (migrations) {
    await applyD1Migrations(
      env.META,
      migrations as unknown as Parameters<typeof applyD1Migrations>[1],
    );
  } else {
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS authors (pubkey BLOB PRIMARY KEY, display_name TEXT UNIQUE, created_at INTEGER NOT NULL, total_uploads INTEGER NOT NULL DEFAULT 0, is_new_creator INTEGER NOT NULL DEFAULT 1, is_denied INTEGER NOT NULL DEFAULT 0)`,
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS artifacts (id TEXT PRIMARY KEY, author_pubkey BLOB NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, content_hash TEXT NOT NULL, thumbnail_hash TEXT NOT NULL, description TEXT, tags TEXT, license TEXT, version TEXT NOT NULL, omni_min_version TEXT NOT NULL, signature BLOB NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, install_count INTEGER NOT NULL DEFAULT 0, report_count INTEGER NOT NULL DEFAULT 0, is_removed INTEGER NOT NULL DEFAULT 0, is_featured INTEGER NOT NULL DEFAULT 0, UNIQUE (author_pubkey, name))`,
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS content_hashes (content_hash TEXT PRIMARY KEY, artifact_id TEXT NOT NULL, first_seen_at INTEGER NOT NULL)`,
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS tombstones (content_hash TEXT PRIMARY KEY, reason TEXT, removed_at INTEGER NOT NULL)`,
    );
    await env.META.exec(
      `CREATE TABLE IF NOT EXISTS install_daily (artifact_id TEXT NOT NULL, day TEXT NOT NULL, install_count INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (artifact_id, day))`,
    );
  }
});

beforeEach(async () => {
  await resetEnv();
});

describe("GET /v1/download/:id — 404 on miss", () => {
  it("returns NOT_FOUND for unknown artifact", async () => {
    const res = await SELF.fetch("https://worker.test/v1/download/does-not-exist");
    expect(res.status).toBe(404);
    const body = (await res.json()) as { error: { code: string } };
    expect(body.error.code).toBe("NOT_FOUND");
  });
});

describe("GET /v1/download/:id — 410 on tombstoned", () => {
  it("returns TOMBSTONED for is_removed=1 artifact", async () => {
    const { id } = await seedArtifact({ isRemoved: true });
    const res = await SELF.fetch(`https://worker.test/v1/download/${id}`);
    expect(res.status).toBe(410);
    const body = (await res.json()) as { error: { code: string } };
    expect(body.error.code).toBe("TOMBSTONED");
  });
});

describe("GET /v1/download/:id — happy path unauthenticated", () => {
  it("returns bundle bytes + contract headers, bumps install counters", async () => {
    const { id, contentHash } = await seedArtifact();
    const res = await SELF.fetch(`https://worker.test/v1/download/${id}`, {
      headers: { "cf-connecting-ip": "192.0.2.1" },
    });
    expect(res.status, await res.clone().text()).toBe(200);
    expect(res.headers.get("content-type")).toContain("application/octet-stream");
    expect(res.headers.get("X-Omni-Content-Hash")).toBe(contentHash);
    expect(res.headers.get("X-Omni-Author-Pubkey")).toBe(PUBKEY_HEX);
    // X-Omni-Signature: the current WASM `WasmSignedBundleHandle` surface does
    // not expose the raw Ed25519 signature, and Worker-repacked blobs strip the
    // JWS entirely (invariant #1). The contract (§4.2) says the header must
    // carry the real signature or be absent — never empty-string, never a
    // placeholder. Assert absence here until the WASM surface grows a
    // `signatureBytes()` accessor.
    expect(res.headers.get("X-Omni-Signature")).toBeNull();
    // X-Omni-Manifest must be a non-empty base64 string whose decode parses as
    // JSON and carries the expected manifest identity fields.
    const manifestB64 = res.headers.get("X-Omni-Manifest");
    expect(manifestB64).toBeTruthy();
    const manifestJson = JSON.parse(atob(manifestB64!)) as Record<string, unknown>;
    expect(manifestJson.name).toBe("download-test");
    expect(manifestJson.version).toBe("1.0.0");
    expect(manifestJson.schema_version).toBe(1);
    expect(Array.isArray(manifestJson.files)).toBe(true);
    const bytes = new Uint8Array(await res.arrayBuffer());
    expect(bytes.byteLength).toBeGreaterThan(0);

    const row = await env.META.prepare(
      "SELECT install_count FROM artifacts WHERE id = ?",
    ).bind(id).first<{ install_count: number }>();
    expect(row?.install_count).toBe(1);

    const day = new Date().toISOString().slice(0, 10);
    const drow = await env.META.prepare(
      "SELECT install_count FROM install_daily WHERE artifact_id = ? AND day = ?",
    ).bind(id, day).first<{ install_count: number }>();
    expect(drow?.install_count).toBe(1);
  });
});

describe("GET /v1/download/:id — rate limit (unauthed, IP-keyed)", () => {
  it("returns 429 RATE_LIMITED after IP minute cap exhausted", async () => {
    const { id } = await seedArtifact();
    const ip = "192.0.2.99";
    const now = new Date();
    const suffix = `${now.getUTCFullYear()}-${String(now.getUTCMonth() + 1).padStart(2, "0")}-${String(now.getUTCDate()).padStart(2, "0")}T${String(now.getUTCHours()).padStart(2, "0")}:${String(now.getUTCMinutes()).padStart(2, "0")}`;
    await env.STATE.put(
      `quota:device:ip_${ip}:${suffix}`,
      String(1_000_000),
      { expirationTtl: 120 },
    );
    const res = await SELF.fetch(`https://worker.test/v1/download/${id}`, {
      headers: { "cf-connecting-ip": ip },
    });
    expect(res.status).toBe(429);
    const body = (await res.json()) as { error: { code: string } };
    expect(body.error.code).toBe("RATE_LIMITED");
  });
});
