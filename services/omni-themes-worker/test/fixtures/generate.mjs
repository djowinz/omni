#!/usr/bin/env node
// Deterministic signed-bundle fixture generator for the Worker test suite.
// Plan #008 W1T3. Dog-foods the WASM `omni_identity.packSignedBundle` export
// built in W1T2, so any parity regression between native Rust and WASM bundle
// packing surfaces here before a single route test runs.
//
// Outputs (all gitignored — regenerate from seed on every CI run):
//   - theme-only.omnipkg        one CSS file, resource_kinds.theme
//   - bundle-with-font.omnipkg  CSS + font entry
//   - bundle-tampered.omnipkg   bundle-with-font with one post-sign byte flip
//   - fixtures.json             index of {pubkey_hex, df_hex, content_hash_hex,
//                                          size_bytes} per fixture
//
// Keypair: TEST_KEYPAIR_SEED = Uint8Array(32).fill(7). Pure test material —
// never used outside fixtures.
// Device fingerprint: sha256(pubkey || "omni-test-df") — deterministic stable
// 32-byte value for test DF anchoring (rate-limiter tests bind counters here).

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { createHash } from "node:crypto";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import initIdentity, * as Identity from "../../src/wasm/omni_identity.js";
import initBundle, * as Bundle from "../../src/wasm/omni_bundle.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const WASM_DIR = resolve(__dirname, "../../src/wasm");

// ---- wasm init (Node: feed raw bytes; `web` target accepts Uint8Array) -----
async function loadWasm(init, filename) {
  const bytes = readFileSync(resolve(WASM_DIR, filename));
  await init({ module_or_path: bytes });
}

// ---- deterministic keypair + DF ---------------------------------------------
const TEST_KEYPAIR_SEED = new Uint8Array(32).fill(7);

// Derive Ed25519 pubkey via Node's Web Crypto (Node 20+). We need the pubkey
// in fixtures.json before packing; the WASM signer derives it internally but
// doesn't expose it separately. Wrapping the seed in a minimal PKCS8 envelope
// is the supported Web Crypto input path for Ed25519.
async function derivePubkeyWebCrypto(seed) {
  // Node's Web Crypto accepts raw Ed25519 private keys as PKCS8. Build one.
  // PKCS8 prefix for Ed25519 private key of 32 bytes:
  const PKCS8_PREFIX = Uint8Array.from([
    0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70,
    0x04, 0x22, 0x04, 0x20,
  ]);
  const pkcs8 = new Uint8Array(PKCS8_PREFIX.length + seed.length);
  pkcs8.set(PKCS8_PREFIX, 0);
  pkcs8.set(seed, PKCS8_PREFIX.length);
  const key = await crypto.subtle.importKey(
    "pkcs8",
    pkcs8,
    { name: "Ed25519" },
    true,
    ["sign"],
  );
  const jwk = await crypto.subtle.exportKey("jwk", key);
  // jwk.x is base64url(pubkey)
  const pad = "=".repeat((4 - (jwk.x.length % 4)) % 4);
  const b64 = (jwk.x + pad).replace(/-/g, "+").replace(/_/g, "/");
  return Uint8Array.from(Buffer.from(b64, "base64"));
}

function deriveDeviceFingerprint(pubkey) {
  return new Uint8Array(
    createHash("sha256").update(pubkey).update("omni-test-df").digest(),
  );
}

const hex = (u8) => Buffer.from(u8).toString("hex");
const sha256 = (u8) => new Uint8Array(createHash("sha256").update(u8).digest());

// ---- manifest builders ------------------------------------------------------
function buildManifest({ name, files, includeFont }) {
  const resource_kinds = {
    theme: {
      dir: "themes/",
      extensions: [".css"],
      max_size_bytes: 1_048_576,
    },
  };
  if (includeFont) {
    resource_kinds.font = {
      dir: "fonts/",
      extensions: [".ttf", ".otf"],
      max_size_bytes: 4_194_304,
    };
  }
  return {
    schema_version: 1,
    name,
    version: "1.0.0",
    omni_min_version: "0.1.0",
    description: `Fixture ${name}`,
    tags: [],
    license: "MIT",
    entry_overlay: "overlay.omni",
    sensor_requirements: [],
    files: files.map((f) => ({
      path: f.path,
      sha256: hex(sha256(f.bytes)),
    })),
    resource_kinds,
  };
}

// Minimal valid overlay.omni payload (treated as opaque text by bundle pack;
// the `entry_overlay` field must point at a file that exists in `files`).
const OVERLAY_BYTES = new TextEncoder().encode(
  '<!doctype html><html><body data-sensor="cpu.usage"></body></html>\n',
);

const THEME_CSS = new TextEncoder().encode(
  "/* omni test fixture theme */\nbody { background: #111; color: #eee; }\n",
);

// Minimal-stub TTF: OTS/Ultralight WILL reject it at sanitize time. That's
// acceptable here — Task 3's scope is fixture-generation determinism, not a
// full OTS-valid font. The sanitize-rejection path is itself a useful test
// case for the font dispatch route. Documented in README.md.
const STUB_TTF = new Uint8Array([
  0x00, 0x01, 0x00, 0x00, // sfnt version (TrueType)
  0x00, 0x00,             // numTables = 0
  0x00, 0x00,             // searchRange
  0x00, 0x00,             // entrySelector
  0x00, 0x00,             // rangeShift
]);

// ---- main -------------------------------------------------------------------
async function main() {
  await loadWasm(initIdentity, "omni_identity.wasm");
  await loadWasm(initBundle, "omni_bundle.wasm");

  const pubkey = await derivePubkeyWebCrypto(TEST_KEYPAIR_SEED);
  const df = deriveDeviceFingerprint(pubkey);

  const outDir = __dirname;
  mkdirSync(outDir, { recursive: true });

  const index = {
    _meta: {
      seed_hex: hex(TEST_KEYPAIR_SEED),
      pubkey_hex: hex(pubkey),
      df_hex: hex(df),
      df_derivation: 'sha256(pubkey || "omni-test-df")',
    },
    fixtures: {},
  };

  // ---- fixture 1: theme-only ----
  {
    const files = [
      { path: "overlay.omni", bytes: OVERLAY_BYTES },
      { path: "themes/default.css", bytes: THEME_CSS },
    ];
    const manifest = {
      ...buildManifest({ name: "theme-only", files, includeFont: false }),
      default_theme: "themes/default.css",
    };
    const filesMap = new Map(files.map((f) => [f.path, f.bytes]));
    const bytes = Identity.packSignedBundle(
      manifest,
      filesMap,
      TEST_KEYPAIR_SEED,
      undefined,
    );
    writeFileSync(resolve(outDir, "theme-only.omnipkg"), bytes);
    const contentHash = Bundle.canonicalHash(manifest);
    index.fixtures["theme-only"] = {
      file: "theme-only.omnipkg",
      content_hash_hex: hex(contentHash),
      size_bytes: bytes.length,
      has_font: false,
      tampered: false,
    };
  }

  // ---- fixture 2: bundle-with-font ----
  let validWithFontBytes;
  let validWithFontManifest;
  {
    const files = [
      { path: "overlay.omni", bytes: OVERLAY_BYTES },
      { path: "themes/default.css", bytes: THEME_CSS },
      { path: "fonts/stub.ttf", bytes: STUB_TTF },
    ];
    const manifest = {
      ...buildManifest({ name: "bundle-with-font", files, includeFont: true }),
      default_theme: "themes/default.css",
    };
    validWithFontManifest = manifest;
    const filesMap = new Map(files.map((f) => [f.path, f.bytes]));
    const bytes = Identity.packSignedBundle(
      manifest,
      filesMap,
      TEST_KEYPAIR_SEED,
      undefined,
    );
    validWithFontBytes = bytes;
    writeFileSync(resolve(outDir, "bundle-with-font.omnipkg"), bytes);
    const contentHash = Bundle.canonicalHash(manifest);
    index.fixtures["bundle-with-font"] = {
      file: "bundle-with-font.omnipkg",
      content_hash_hex: hex(contentHash),
      size_bytes: bytes.length,
      has_font: true,
      tampered: false,
    };
  }

  // ---- fixture 3: bundle-tampered ----
  // Flip one byte inside the CSS entry's compressed data. The zip local-file
  // header starts with 'PK\x03\x04'; scan past the first one and flip a byte
  // well past the header + filename region. Any byte inside the covered
  // payload will trip either the per-file sha256 check or the canonical-hash
  // JWS verification.
  {
    const tampered = new Uint8Array(validWithFontBytes);
    // Target: find the last 'PK\x01\x02' (central directory header) and step
    // backward 32 bytes into compressed content — reliably inside a file
    // entry's data region on a small bundle.
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
    if (cdOffset < 64) {
      throw new Error(
        `tamper: could not locate central directory (cdOffset=${cdOffset})`,
      );
    }
    const flipAt = cdOffset - 32;
    tampered[flipAt] ^= 0xff;
    writeFileSync(resolve(outDir, "bundle-tampered.omnipkg"), tampered);

    // Verify tampered bundle is rejected by unpackSignedBundle.
    let rejected = false;
    let rejectionMessage = null;
    try {
      const handle = Identity.unpackSignedBundle(tampered, undefined);
      // Drain files to force any deferred integrity check.
      while (handle.nextFile() !== null) {
        // drain
      }
      handle.free?.();
    } catch (e) {
      rejected = true;
      rejectionMessage = e?.message ?? String(e);
    }

    index.fixtures["bundle-tampered"] = {
      file: "bundle-tampered.omnipkg",
      content_hash_hex: hex(Bundle.canonicalHash(validWithFontManifest)),
      size_bytes: tampered.length,
      has_font: true,
      tampered: true,
      tamper_offset: flipAt,
      tamper_rejected_on_unpack: rejected,
      tamper_rejection_message: rejectionMessage,
    };

    if (!rejected) {
      console.warn(
        "[fixtures] WARNING: tampered bundle was NOT rejected by " +
          "unpackSignedBundle. Surface as DONE_WITH_CONCERNS.",
      );
    }
  }

  writeFileSync(
    resolve(outDir, "fixtures.json"),
    JSON.stringify(index, null, 2) + "\n",
  );

  console.log("[fixtures] generated:");
  for (const [k, v] of Object.entries(index.fixtures)) {
    console.log(
      `  ${k.padEnd(20)} ${String(v.size_bytes).padStart(6)}B  ` +
        `hash=${v.content_hash_hex.slice(0, 16)}…`,
    );
  }
}

main().catch((e) => {
  console.error("[fixtures] failed:", e);
  process.exit(1);
});
