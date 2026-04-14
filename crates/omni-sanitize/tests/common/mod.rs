//! Shared fixture builders for sanitize tests.

use omni_bundle::{FileEntry, Manifest};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn sha256(b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b);
    h.finalize().into()
}

pub fn bundle_with_overlay_bytes(
    bytes: Vec<u8>,
) -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), bytes.clone());
    let manifest = Manifest {
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
        files: vec![FileEntry { path: "overlay.omni".into(), sha256: sha256(&bytes) }],
        resource_kinds: None,
    };
    (manifest, files)
}

pub fn bundle_with_image(
    path: &str,
    bytes: Vec<u8>,
) -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let overlay = br#"<overlay><template><div/></template></overlay>"#.to_vec();
    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), overlay.clone());
    files.insert(path.to_string(), bytes.clone());
    let manifest = Manifest {
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
        files: vec![
            FileEntry { path: "overlay.omni".into(), sha256: sha256(&overlay) },
            FileEntry { path: path.into(), sha256: sha256(&bytes) },
        ],
        resource_kinds: None,
    };
    (manifest, files)
}
