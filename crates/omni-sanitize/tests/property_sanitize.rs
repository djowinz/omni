//! Property-based no-panic / no-third-party-leakage invariants on sanitize_bundle.

use omni_bundle::{FileEntry, Manifest};
use omni_sanitize::sanitize_bundle;
use proptest::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn sha256(b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b);
    h.finalize().into()
}

fn manifest_with(files: &BTreeMap<String, Vec<u8>>) -> Manifest {
    Manifest {
        schema_version: 1,
        name: "t".into(),
        version: semver::Version::new(0, 1, 0),
        omni_min_version: semver::Version::new(0, 1, 0),
        description: String::new(),
        tags: vec![],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: None,
        sensor_requirements: vec![],
        files: files.iter().map(|(p, b)| FileEntry { path: p.clone(), sha256: sha256(b) }).collect(),
        resource_kinds: None,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn no_panic_on_mutated_theme(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
        let mut files = BTreeMap::new();
        files.insert("overlay.omni".to_string(), br#"<overlay><template/></overlay>"#.to_vec());
        files.insert("themes/x.css".to_string(), bytes);
        let manifest = manifest_with(&files);
        let r = sanitize_bundle(&manifest, files);
        match r {
            Ok(_) => {},
            Err(e) => {
                let s = format!("{e}");
                assert!(!s.contains("zip::"), "leaked zip: {s}");
                assert!(!s.contains("serde_json::"), "leaked serde_json: {s}");
                assert!(!s.contains("lightningcss::"), "leaked lightningcss: {s}");
            }
        }
    }

    #[test]
    fn no_panic_on_random_image(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
        let mut files = BTreeMap::new();
        files.insert("overlay.omni".to_string(), br#"<overlay><template/></overlay>"#.to_vec());
        files.insert("images/x.png".to_string(), bytes);
        let manifest = manifest_with(&files);
        let _ = sanitize_bundle(&manifest, files);
    }
}
