import { describe, it, expect, beforeAll } from "vitest";
import { env } from "cloudflare:test";
import type { Env } from "../src/env";
import { loadWasm } from "../src/lib/wasm";

/**
 * Tier B — BundleProcessor Durable Object under Miniflare.
 *
 * Fixtures are generated inline using the same WASM `packSignedBundle` export
 * the on-disk fixtures use (`test/fixtures/generate.mjs`), because the
 * vitest-pool-workers runtime runs tests inside the Workers isolate where
 * `node:fs` is not available. Inline generation also lets us vary payloads
 * per-test (oversize fonts, MZ magic, …) without recompiling fixtures.
 *
 * Coverage:
 *   - theme-only happy path (inline-size bundle with only CSS)
 *   - bundle-with-font dispatch (the stub font is intentionally OTS-rejecting;
 *     we assert Unsafe classification, mirroring the W1T3 fixture contract)
 *   - executable-magic in a font slot → Unsafe.RejectedExecutableMagic
 *   - oversized single file → SIZE_EXCEEDED (413)
 *   - tampered bundle → Integrity rejection (ZipBomb guard or JWS mismatch)
 *   - empty body rejection
 */

declare module "cloudflare:test" {
  interface ProvidedEnv extends Env {}
}

// ---------------------------------------------------------------------------
// Fixture key material — matches test/fixtures/fixtures.json (seed 0x07 rep).
// ---------------------------------------------------------------------------
const SEED_HEX = "0707070707070707070707070707070707070707070707070707070707070707";

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}
function bytesToHex(b: Uint8Array): string {
  let s = "";
  for (let i = 0; i < b.length; i++) s += b[i]!.toString(16).padStart(2, "0");
  return s;
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", bytes);
  return bytesToHex(new Uint8Array(d));
}

const SEED = hexToBytes(SEED_HEX);

// Minimal valid overlay: the sanitize handler requires a <overlay>-rooted
// XML document (see crates/omni-sanitize/src/handlers/overlay.rs). The W1T3
// on-disk fixture uses an HTML-doctype variant that is NEVER put through the
// full sanitize pipeline in existing tests — we need a real overlay payload
// here so the theme-only happy-path actually reaches a 200 response.
const OVERLAY_BYTES = new TextEncoder().encode(
  '<overlay><template><div data-sensor="cpu.usage"/></template></overlay>',
);
const THEME_CSS_BYTES = new TextEncoder().encode(
  "/* test */\nbody { background: #111; color: #eee; }\n",
);
const STUB_TTF = new Uint8Array([
  0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
]);

interface BundleOpts {
  includeFont?: boolean;
  fontBytes?: Uint8Array;
  fontMaxSize?: number;
  themeMaxSize?: number;
  overrideThemeCss?: Uint8Array;
}

async function buildSignedBundle(opts: BundleOpts = {}): Promise<Uint8Array> {
  const { identity } = await loadWasm();
  const includeFont = opts.includeFont ?? false;

  const entries: Array<{ path: string; bytes: Uint8Array }> = [
    { path: "overlay.omni", bytes: OVERLAY_BYTES },
    { path: "themes/default.css", bytes: opts.overrideThemeCss ?? THEME_CSS_BYTES },
  ];
  if (includeFont) {
    entries.push({
      path: "fonts/stub.ttf",
      bytes: opts.fontBytes ?? STUB_TTF,
    });
  }

  const manifest: Record<string, unknown> = {
    schema_version: 1,
    name: "inline-do-test",
    version: "1.0.0",
    omni_min_version: "0.1.0",
    description: "inline fixture for do.test.ts",
    tags: [],
    license: "MIT",
    entry_overlay: "overlay.omni",
    default_theme: "themes/default.css",
    sensor_requirements: [],
    files: await Promise.all(
      entries.map(async (f) => ({
        path: f.path,
        sha256: await sha256Hex(f.bytes),
      })),
    ),
    resource_kinds: {
      // `dir` is matched exactly against the path segment (see
      // crates/omni-sanitize/src/handlers/mod.rs::matches_dir_ext) — must NOT
      // include a trailing slash, else it falls through to the default
      // handler whose default_max_size silently shadows the declared cap.
      theme: {
        dir: "themes",
        extensions: [".css"],
        max_size_bytes: opts.themeMaxSize ?? 1_048_576,
      },
      ...(includeFont
        ? {
            font: {
              dir: "fonts",
              extensions: [".ttf", ".otf"],
              max_size_bytes: opts.fontMaxSize ?? 4_194_304,
            },
          }
        : {}),
    },
  };

  const filesMap = new Map(entries.map((f) => [f.path, f.bytes] as const));
  return identity.packSignedBundle(manifest, filesMap, SEED, undefined);
}

async function callDO(bundleBytes: Uint8Array, dfHex: string): Promise<Response> {
  const id = env.BUNDLE_PROCESSOR.idFromName(dfHex);
  const stub = env.BUNDLE_PROCESSOR.get(id);
  return stub.fetch("https://do.internal/sanitize", {
    method: "POST",
    headers: { "content-type": "application/octet-stream" },
    body: bundleBytes,
  });
}

describe("BundleProcessor DO — sanitize pipeline", () => {
  beforeAll(async () => {
    // Warm the WASM cache inside the test isolate; the DO isolate warms on
    // first fetch. Surfacing any binding error here avoids cross-isolate
    // debugging noise if the WASM glue breaks.
    await loadWasm();
  });

  it("happy path: theme-only bundle is sanitized and repacked", async () => {
    const bytes = await buildSignedBundle({ includeFont: false });
    const res = await callDO(bytes, "df-theme-happy");
    expect(res.status, await res.clone().text()).toBe(200);
    const body = (await res.json()) as {
      sanitized_bundle: string;
      sanitize_report: { version: number; files: unknown[] };
      canonical_hash: string;
    };
    expect(typeof body.sanitized_bundle).toBe("string");
    expect(body.sanitized_bundle.length).toBeGreaterThan(0);
    expect(body.sanitize_report.version).toBe(1);
    expect(Array.isArray(body.sanitize_report.files)).toBe(true);
    expect(body.canonical_hash).toMatch(/^[0-9a-f]{64}$/);
  });

  it("bundle-with-font dispatches sanitize (stub TTF is rejected by OTS as Unsafe)", async () => {
    // The stub font is OTS-unparseable by design (numTables=0). That matches
    // the on-disk W1T3 fixture contract documented in test/fixtures/README.md
    // "Font fixture caveat" — this test proves the DO classifies the handler
    // rejection as Unsafe (422) rather than a 500 or an Integrity mismatch.
    const bytes = await buildSignedBundle({ includeFont: true });
    const res = await callDO(bytes, "df-font-dispatch");
    expect(res.status, await res.clone().text()).toBe(422);
    const body = (await res.json()) as { kind?: string };
    expect(body.kind).toBe("Unsafe");
  });

  it("executable magic (MZ prefix) in a font slot → Unsafe.RejectedExecutableMagic", async () => {
    const evil = new Uint8Array(1024);
    evil[0] = 0x4d; // 'M'
    evil[1] = 0x5a; // 'Z'
    for (let i = 2; i < evil.length; i++) evil[i] = i & 0xff;

    const bytes = await buildSignedBundle({ includeFont: true, fontBytes: evil });
    const res = await callDO(bytes, "df-exec-magic");
    expect(res.status, await res.clone().text()).toBe(422);
    const body = (await res.json()) as { kind?: string; detail?: string };
    expect(body.kind).toBe("Unsafe");
    expect(body.detail).toBe("RejectedExecutableMagic");
  });

  it("oversized single file is rejected with SIZE_EXCEEDED", async () => {
    // Inflate the CSS past the DEFAULT theme handler cap (131072 bytes) so
    // the size-exceeded check fires regardless of whether the dispatch uses
    // the declared resource_kinds or falls through to the built-in default.
    // The size check runs before any per-kind handler sanitize (see
    // crates/omni-sanitize/src/lib.rs main loop).
    // Use pseudo-random bytes so DEFLATE can't compress them — the unpack
    // ZipBomb guard (compressed_ratio > N) fires on highly-compressible
    // payloads before the per-file size check runs, so repeated-padding
    // content would never reach the sanitize stage.
    const size = 140_000; // > 131_072 default theme cap
    const bigCss = new Uint8Array(size);
    // crypto.getRandomValues yields high-entropy bytes DEFLATE can't squeeze —
    // avoids hitting the ZipBomb guard before the size-exceeded check fires.
    // Web Crypto caps each call at 65536 bytes, so chunk through the buffer.
    for (let off = 0; off < size; off += 65536) {
      crypto.getRandomValues(bigCss.subarray(off, Math.min(off + 65536, size)));
    }
    // Keep a CSS-ish prefix so file-type dispatch still treats it as theme.
    const prefix = new TextEncoder().encode("body{}\n");
    bigCss.set(prefix, 0);
    const bytes = await buildSignedBundle({
      includeFont: false,
      overrideThemeCss: bigCss,
    });
    const res = await callDO(bytes, "df-oversize");
    // 413 per worker-api.md §3 SizeExceeded row; body carries kind=Malformed.
    expect(res.status, await res.clone().text()).toBe(413);
    const body = (await res.json()) as {
      error: { code: string };
      kind?: string;
    };
    expect(body.error.code).toBe("SIZE_EXCEEDED");
    expect(body.kind).toBe("Malformed");
  });

  it("tampered bundle → Integrity rejection (ZipBomb guard or JWS mismatch)", async () => {
    // Mirror the W1T3 tampering pattern: flip a byte inside the zip payload.
    // The fixture README notes this trips ZipBomb before the JWS check; we
    // assert "Integrity" without pinning the sub-kind, which matches both
    // classification paths.
    const valid = await buildSignedBundle({ includeFont: true });
    const tampered = new Uint8Array(valid);
    // Find the central-directory header (PK\x01\x02) and flip 32 bytes
    // before it — reliably inside compressed payload on a small bundle.
    let cdOffset = -1;
    for (let i = tampered.length - 4; i >= 0; i--) {
      if (
        tampered[i] === 0x50 &&
        tampered[i + 1] === 0x4b &&
        tampered[i + 2] === 0x01 &&
        tampered[i + 3] === 0x02
      ) {
        cdOffset = i;
        break;
      }
    }
    expect(cdOffset).toBeGreaterThan(64);
    tampered[cdOffset - 32] ^= 0xff;

    const res = await callDO(tampered, "df-tampered");
    expect(res.status, await res.clone().text()).toBe(422);
    const body = (await res.json()) as { kind?: string };
    expect(body.kind).toBe("Integrity");
  });

  it("rejects empty body with 400 BAD_REQUEST", async () => {
    const res = await callDO(new Uint8Array(0), "df-empty");
    expect(res.status).toBe(400);
    const body = (await res.json()) as { error: { code: string } };
    expect(body.error.code).toBe("BAD_REQUEST");
  });
});
