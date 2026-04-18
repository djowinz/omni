/**
 * Canonical-hash WASM↔native parity (umbrella §8.2, plan #008 W4T15).
 *
 * Invariant #6: canonical hash = SHA-256(RFC 8785 JCS manifest bytes). The
 * Rust-side test `crates/bundle/tests/hash.rs::canonical_hash_matches_golden`
 * asserts native parity against the same committed golden fixtures:
 *   - `crates/bundle/tests/fixtures/golden-manifest.json` — the JCS
 *     canonical bytes of a known manifest (byte-stable, includable as-is).
 *   - `crates/bundle/tests/fixtures/golden-hash.hex` — the expected
 *     SHA-256 hex of those bytes.
 *
 * This file proves WASM agrees by performing a three-way assertion:
 *   1. WASM `omni_bundle.canonicalHash(parsedManifest)` hex == golden hex.
 *   2. Direct `crypto.subtle.digest("SHA-256", rawManifestBytes)` hex ==
 *      golden hex. This guards that `golden-manifest.json` really is the
 *      RFC 8785 canonical form — if the file drifts, parsing + re-hashing
 *      via WASM could still match a stale golden; the raw-bytes hash would
 *      not. Both assertions must hold.
 *   3. WASM hex == direct hex (transitively), fully closing the loop.
 *
 * If (1) fails, that is a real WASM bug — DO NOT regenerate the golden.
 * Surface BLOCKED per plan guidance.
 */
import { describe, it, expect } from "vitest";
import { canonicalHash } from "../src/lib/canonical";
import { loadWasm } from "../src/lib/wasm";


// The workerd-based test pool does not implement `node:fs`, so we load the
// golden fixtures via Vite's `?raw` asset transform at build time. The two
// files live under the workspace root in the Rust crate's test tree — the
// same bytes the Rust-side `canonical_hash_matches_golden` test consumes
// via `include_bytes!` / `include_str!`. If either path drifts, the Vite
// transform will fail to resolve at collect time rather than at runtime.
//
// `?raw` yields the UTF-8 decoded contents as a string. For the manifest
// we re-encode to bytes via TextEncoder; RFC 8785 JCS output is ASCII-only
// (no escapes that would require special handling), so the round-trip is
// byte-identical to the on-disk file.
// TypeScript doesn't know about Vite's `?raw` asset transform without an
// ambient declaration, and adding a `.d.ts` in this wave would overlap
// another task's file ownership. The @ts-expect-error pins the suppression
// to these two lines — if a future config makes `?raw` resolvable in the
// type system, tsc will flag the directives as unused and prompt removal.
// @ts-expect-error — `?raw` is a Vite import suffix handled at transform time.
import manifestRaw from "../../../crates/bundle/tests/fixtures/golden-manifest.json?raw";
// @ts-expect-error — `?raw` is a Vite import suffix handled at transform time.
import hashRaw from "../../../crates/bundle/tests/fixtures/golden-hash.hex?raw";

function toHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

describe("canonical_hash WASM↔native parity (umbrella §8.2)", () => {
  it("wasm canonicalHash(golden-manifest) matches committed golden hex", async () => {
    await loadWasm();

    // Raw bytes = the RFC 8785 JCS canonical form on disk.
    const manifestBytes = new TextEncoder().encode(manifestRaw);
    const manifest = JSON.parse(manifestRaw);
    const goldenHex = hashRaw.trim();

    // (1) WASM over parsed-then-re-canonicalized manifest.
    const wasmHash = await canonicalHash(manifest);
    expect(wasmHash).toBeInstanceOf(Uint8Array);
    expect(wasmHash.length).toBe(32);
    const wasmHex = toHex(wasmHash);

    // (2) SHA-256 of the on-disk canonical bytes directly.
    const directDigest = new Uint8Array(
      await crypto.subtle.digest("SHA-256", manifestBytes),
    );
    const directHex = toHex(directDigest);

    // Diagnostic — surfaces all three when a failure lands in CI logs.
    // eslint-disable-next-line no-console
    console.log(
      `[canonical_parity] golden=${goldenHex}\n[canonical_parity] wasm  =${wasmHex}\n[canonical_parity] direct=${directHex}`,
    );

    // (3) Three-way agreement. If WASM drifts, fail loudly — do NOT update
    // the golden to match. Regenerating the golden is exclusively a #005
    // Rust-fixture-regen task.
    expect(directHex).toBe(goldenHex);
    expect(wasmHex).toBe(goldenHex);
    expect(wasmHex).toBe(directHex);
  });
});
