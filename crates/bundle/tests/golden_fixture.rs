//! Canonical-hash golden fixture.
//!
//! Two files in `tests/fixtures/` lock the canonical-hash byte format:
//! - `golden-manifest.json` — RFC 8785 JCS serialization of a known manifest.
//! - `golden-hash.hex` — SHA-256 of the above file's bytes, 64 lowercase hex.
//!
//! Cross-implementation contract: the Worker's TypeScript / WASM copy of
//! `canonical_hash` must reproduce the same hex output when given the same JSON
//! bytes. Consumed by theme-sharing sub-spec #008's WASM-parity integration
//! test. Authoritative algorithm spec:
//! `docs/contracts/canonical-hash-algorithm.md`.
//!
//! To regenerate (only when the `sample_manifest()` shape or the hash algorithm
//! legitimately changes):
//! ```text
//! WRITE_GOLDEN=1 cargo test -p omni-bundle --test golden_fixture
//! ```
//! and commit the updated fixture files. Every normal `cargo test` run asserts
//! the fixture bytes match `serde_jcs::to_vec(&sample_manifest())` so drift
//! fails loudly.

use std::collections::BTreeMap;
use std::path::PathBuf;

use omni_bundle::{canonical_hash, FileEntry, Manifest, Tag};
use sha2::{Digest, Sha256};

fn sample_manifest() -> Manifest {
    // Intentionally identical to `crates/omni-bundle/src/hash.rs::tests::sample()`
    // so the Rust unit test and this fixture file exercise the same input.
    Manifest {
        schema_version: 1,
        name: "Sample".into(),
        version: "1.0.0".parse().unwrap(),
        omni_min_version: "0.1.0".parse().unwrap(),
        description: "d".into(),
        tags: vec![Tag::new("dark").unwrap()],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/default.css".into()),
        sensor_requirements: vec![],
        files: vec![
            FileEntry { path: "overlay.omni".into(), sha256: [0u8; 32] },
            FileEntry { path: "themes/default.css".into(), sha256: [0u8; 32] },
        ],
        resource_kinds: None,
    }
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn json_path() -> PathBuf { fixtures_dir().join("golden-manifest.json") }
fn hex_path() -> PathBuf { fixtures_dir().join("golden-hash.hex") }

#[test]
fn golden_fixture_matches_canonical_output() {
    let manifest = sample_manifest();
    let canonical_bytes = serde_jcs::to_vec(&manifest)
        .expect("canonical serialization must not fail for a validated manifest");
    let expected_hash: [u8; 32] = Sha256::digest(&canonical_bytes).into();
    let expected_hex = hex::encode(expected_hash);

    if std::env::var("WRITE_GOLDEN").is_ok() {
        std::fs::create_dir_all(fixtures_dir()).unwrap();
        std::fs::write(json_path(), &canonical_bytes).unwrap();
        std::fs::write(hex_path(), format!("{expected_hex}\n")).unwrap();
        eprintln!("wrote {} ({} bytes) and {}", json_path().display(), canonical_bytes.len(), hex_path().display());
    }

    // Always assert — this test fails if the fixtures drift from sample_manifest()
    // or if the canonical_hash algorithm changes semantics.
    let json_on_disk = std::fs::read(json_path()).expect(
        "golden-manifest.json missing; run `WRITE_GOLDEN=1 cargo test -p omni-bundle --test golden_fixture` to generate",
    );
    let hex_on_disk = std::fs::read_to_string(hex_path()).expect(
        "golden-hash.hex missing; run with WRITE_GOLDEN=1 to generate",
    );
    let hex_on_disk = hex_on_disk.trim();

    assert_eq!(json_on_disk, canonical_bytes,
        "golden-manifest.json does not match serde_jcs::to_vec(sample_manifest()). \
         Either sample_manifest() changed or the JCS serializer did — regenerate with WRITE_GOLDEN=1.");

    assert_eq!(hex_on_disk, expected_hex,
        "golden-hash.hex does not match SHA-256 of the canonical bytes. Regenerate with WRITE_GOLDEN=1.");

    // Belt-and-suspenders: the shipped canonical_hash() function must reproduce
    // the same hash from the Manifest struct (independent of the file bytes).
    let files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let computed = canonical_hash(&manifest, &files);
    assert_eq!(hex::encode(computed), expected_hex,
        "canonical_hash() disagrees with SHA-256(JCS bytes) — the two paths have diverged.");
}
