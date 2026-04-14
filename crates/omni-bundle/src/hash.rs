use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::manifest::{canonical_manifest_bytes, Manifest};

/// SHA-256 of a byte slice. Shared across pack / unpack to avoid duplicated
/// inline implementations.
pub(crate) fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Canonical content hash for a bundle. Computed as
/// `SHA-256(serde_jcs::to_vec(manifest))` — the manifest embeds every
/// `FileEntry.sha256`, so the manifest is itself a Merkle root over the
/// bundle's content. The `files` parameter is accepted for API symmetry
/// with earlier versions but intentionally unused.
///
/// Authoritative algorithm specification:
/// `docs/superpowers/specs/contracts/canonical-hash-algorithm.md`
pub fn canonical_hash(manifest: &Manifest, _files: &BTreeMap<String, Vec<u8>>) -> [u8; 32] {
    let bytes = canonical_manifest_bytes(manifest)
        .expect("canonical manifest serialization must not fail for a validated manifest");
    sha256_of(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{FileEntry, Manifest, Tag};

    fn sample() -> (Manifest, BTreeMap<String, Vec<u8>>) {
        let mut files = BTreeMap::new();
        files.insert("overlay.omni".into(), b"<overlay/>".to_vec());
        files.insert("themes/default.css".into(), b":root{--a:0}".to_vec());

        let m = Manifest {
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
        };
        (m, files)
    }

    #[test]
    fn canonical_hash_is_stable() {
        let (m, f) = sample();
        assert_eq!(canonical_hash(&m, &f), canonical_hash(&m, &f));
    }

    #[test]
    fn canonical_hash_changes_when_manifest_changes() {
        let (mut m, f) = sample();
        let before = canonical_hash(&m, &f);
        m.name = "Different".into();
        let after = canonical_hash(&m, &f);
        assert_ne!(before, after);
    }

    #[test]
    fn canonical_hash_ignores_files_map() {
        // New design: hash is over manifest only. Changing files without
        // updating their FileEntry.sha256 in the manifest does NOT change the hash.
        // The manifest is the Merkle root.
        let (m, mut f) = sample();
        let before = canonical_hash(&m, &f);
        f.insert("overlay.omni".into(), b"<modified/>".to_vec());
        let after = canonical_hash(&m, &f);
        assert_eq!(before, after, "hash depends on manifest only, not files map");
    }

    #[test]
    fn canonical_hash_changes_when_file_entry_hash_changes() {
        // Proves the hash is sensitive to the Merkle payload, not just top-level
        // string fields. Mutating a FileEntry.sha256 must yield a different hash.
        let (mut m, f) = sample();
        let before = canonical_hash(&m, &f);
        m.files[0].sha256 = [0x42u8; 32];
        let after = canonical_hash(&m, &f);
        assert_ne!(before, after);
    }

    #[test]
    #[ignore = "golden regenerated in Task 9 after full D1–D11 refactor"]
    fn canonical_hash_matches_golden() {
        // Intentionally `todo!` — if someone runs `cargo test -- --ignored`
        // before Task 9 regenerates the expected hash, this must fail loudly
        // rather than silently pass. Task 9 replaces this body with the real
        // golden value.
        let (_m, _f) = sample();
        todo!("golden hash is regenerated in Task 9 after all retro refactors land");
    }
}
