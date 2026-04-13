use std::collections::BTreeMap;

use omni_bundle::{FileEntry, Manifest, Tag};
use sha2::{Digest, Sha256};

#[allow(dead_code)]
pub fn sha256(b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b);
    let out = h.finalize();
    let mut d = [0u8; 32];
    d.copy_from_slice(&out);
    d
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
        tags: vec![Tag::Dark, Tag::HighContrast],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/default.css".into()),
        sensor_requirements: vec!["cpu.usage".into()],
        files: vec![
            FileEntry { path: "overlay.omni".into(), sha256: sha256(&overlay) },
            FileEntry { path: "themes/default.css".into(), sha256: sha256(&css) },
        ],
        signature: None,
    };
    (manifest, files)
}
