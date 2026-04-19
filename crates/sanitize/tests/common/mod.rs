//! Shared fixture builders for sanitize tests.
#![allow(dead_code)]

use bundle::{FileEntry, Manifest};
use identity::Keypair;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn sha256(b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b);
    h.finalize().into()
}

fn bundle_with_asset(path: &str, bytes: Vec<u8>) -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let overlay = br#"<widget><template><div/></template></widget>"#.to_vec();
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
            FileEntry {
                path: "overlay.omni".into(),
                sha256: sha256(&overlay),
            },
            FileEntry {
                path: path.into(),
                sha256: sha256(&bytes),
            },
        ],
        resource_kinds: None,
    };
    (manifest, files)
}

pub fn bundle_with_font(path: &str, bytes: Vec<u8>) -> (Manifest, BTreeMap<String, Vec<u8>>) {
    bundle_with_asset(path, bytes)
}

pub fn bundle_with_image(path: &str, bytes: Vec<u8>) -> (Manifest, BTreeMap<String, Vec<u8>>) {
    bundle_with_asset(path, bytes)
}

pub fn bundle_with_overlay_bytes(bytes: Vec<u8>) -> (Manifest, BTreeMap<String, Vec<u8>>) {
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
        files: vec![FileEntry {
            path: "overlay.omni".into(),
            sha256: sha256(&bytes),
        }],
        resource_kinds: None,
    };
    (manifest, files)
}

/// Valid bundle with one overlay + one theme. For integration roundtrip tests.
pub fn clean_bundle() -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let overlay =
        br#"<widget><template><div class="x"/></template><style>body{}</style></widget>"#
            .to_vec();
    let css = b"body{color:red}".to_vec();
    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), overlay.clone());
    files.insert("themes/default.css".to_string(), css.clone());
    let manifest = Manifest {
        schema_version: 1,
        name: "t".into(),
        version: semver::Version::new(0, 1, 0),
        omni_min_version: semver::Version::new(0, 1, 0),
        description: String::new(),
        tags: vec![],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/default.css".into()),
        sensor_requirements: vec![],
        files: vec![
            FileEntry {
                path: "overlay.omni".into(),
                sha256: sha256(&overlay),
            },
            FileEntry {
                path: "themes/default.css".into(),
                sha256: sha256(&css),
            },
        ],
        resource_kinds: None,
    };
    (manifest, files)
}

pub fn two_keypairs() -> (Keypair, Keypair) {
    (Keypair::generate(), Keypair::generate())
}
