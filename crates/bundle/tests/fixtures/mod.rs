use std::collections::BTreeMap;

use bundle::{FileEntry, Manifest, Tag};
use sha2::{Digest, Sha256};

#[allow(dead_code)]
pub fn sha256(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}

/// Standard zip options used throughout the negative-test suite. Deterministic
/// DateTime + Deflated matches `pack`'s own settings, so tests build bundles
/// shaped like real ones.
#[allow(dead_code)]
pub fn test_zip_opts() -> zip::write::FileOptions {
    zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
}

#[allow(dead_code)]
pub fn sample_bundle() -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let overlay = b"<overlay/>".to_vec();
    let css = b":root{--a:0}".to_vec();

    let mut files = BTreeMap::new();
    files.insert("overlay.omni".into(), overlay.clone());
    files.insert("themes/default.css".into(), css.clone());

    let manifest = Manifest {
        schema_version: 1,
        name: "Sample".into(),
        version: "1.0.0".parse().unwrap(),
        omni_min_version: "0.1.0".parse().unwrap(),
        description: "d".into(),
        tags: vec![
            Tag::new("dark").unwrap(),
            Tag::new("high-contrast").unwrap(),
        ],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/default.css".into()),
        sensor_requirements: vec!["cpu.usage".into()],
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
