# Canonical hash algorithm

**Status:** Authoritative (Phase 0, `schema_version = 1`). Changes require umbrella update + new `schema_version`.
**Applies to:** any implementation that computes a content-addressed hash for an Omni bundle or theme artifact — native Rust (`crates/bundle`), Worker WASM (`apps/worker`), and any future reimplementation.

**Why this file exists:** dedup across host and Worker depends on byte-identical hash output. This algorithm is the cross-language interop contract. Drift between implementations breaks dedup and signature verification silently.

---

## Algorithm (schema_version = 1)

```
canonical_hash(manifest) = SHA-256( serde_jcs::to_vec(manifest) )
```

1. **Input:** a `Manifest` struct (see `bundle-manifest.schema.json`).
2. **Serialize:** RFC 8785 JSON Canonicalization Scheme (JCS). Rust: `serde_jcs` crate. TypeScript: `canonicalize` npm package.
3. **Hash:** SHA-256 of the UTF-8 canonical JSON bytes.
4. **Output:** 32 bytes. Typically rendered as 64 lowercase hex characters.

The manifest's `files` array embeds `FileEntry.sha256` (per-file SHA-256 over raw file bytes). The manifest itself is therefore a Merkle root over the bundle contents — no separate artifact-level hash is needed.

---

## Non-rules (don't do this)

- **Do not hand-roll JSON canonicalization.** RFC 8785 is subtle (number normalization, escape rules, key-ordering, UTF-8 validation). Rust and TypeScript implementations will drift.
- **Do not hash the zip bytes.** The zip format has non-deterministic fields (timestamps, compression metadata) that break byte-identity across implementations.
- **Do not include the signature in the hash input.** Signatures are stored sibling to the hashed artifact per architectural invariant #6a; the hash input is unambiguously "the manifest bytes, full stop."

---

## Version axis

There is one axis: `Manifest.schema_version: u32` (currently `1`). Any change to the hashing algorithm — new canonicalization, different hash function, different input shape — bumps this integer and creates a new `canonical-hash-algorithm-vN.md` document. Parallel version fields (algorithm version, canonicalization version, etc.) do not exist. See architectural invariant #6b.

---

## Test vectors

Canonical fixture (match across implementations):

- **Golden manifest:** `crates/bundle/tests/fixtures/golden-manifest.json` (RFC 8785 JCS output of a known `sample()` manifest with zero-filled per-file hashes)
- **Golden hash:** `crates/bundle/tests/fixtures/golden-hash.hex` — `a31d0150e9817450f012a2a8941e3232a0b1527dab386d6a34312f265ee4c548`

Every implementation (Rust native, Rust wasm, future TypeScript edge) MUST reproduce this hex given the JSON file bytes. CI gate: `crates/bundle/tests/golden_fixture.rs` — fails loudly if drift appears; regenerate fixtures via `WRITE_GOLDEN=1 cargo test -p omni-bundle --test golden_fixture` when the sample or algorithm legitimately changes.
