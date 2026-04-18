//! Reachability test per public SanitizeError variant.

use omni_bundle::{FileEntry, Manifest, ResourceKind};
use omni_sanitize::{sanitize_bundle, sanitize_theme, SanitizeError};
use std::collections::BTreeMap;

mod common;
use common::sha256;

fn minimal_manifest(files: &BTreeMap<String, Vec<u8>>, schema: u32) -> Manifest {
    Manifest {
        schema_version: schema,
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

#[test]
fn reaches_malformed_on_bad_schema_version() {
    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), br#"<overlay><template/></overlay>"#.to_vec());
    let manifest = minimal_manifest(&files, 999);
    let err = sanitize_bundle(&manifest, files).unwrap_err();
    assert!(matches!(err, SanitizeError::Malformed { .. }));
}

#[test]
fn reaches_rejected_executable_magic_via_theme() {
    let mut b = vec![0x4D, 0x5A];
    b.extend_from_slice(b"body{}");
    let err = sanitize_theme(&b).unwrap_err();
    assert!(matches!(err, SanitizeError::RejectedExecutableMagic { .. }));
}

#[test]
fn reaches_unknown_resource_kind_via_declared_unknown() {
    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), br#"<overlay><template/></overlay>"#.to_vec());
    files.insert("sounds/x.wav".to_string(), b"RIFF____WAVE".to_vec());
    let mut rk = BTreeMap::new();
    rk.insert("sounds".to_string(), ResourceKind {
        dir: "sounds".into(),
        extensions: vec!["wav".into()],
        max_size_bytes: 1024,
    });
    let mut m = minimal_manifest(&files, 1);
    m.resource_kinds = Some(rk);
    let err = sanitize_bundle(&m, files).unwrap_err();
    match err {
        SanitizeError::UnknownResourceKind { kind, supported } => {
            assert_eq!(kind, "sounds");
            assert!(supported.contains(&"theme"));
        }
        other => panic!("expected UnknownResourceKind, got {other:?}"),
    }
}

#[test]
fn reaches_handler_error_via_bad_css() {
    let err = sanitize_theme(b"@import 'x.css'; body{}").unwrap_err();
    assert!(matches!(err, SanitizeError::Handler { kind: "theme", .. }));
}

#[test]
fn reaches_size_exceeded() {
    let css = b"body{}".to_vec();
    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), br#"<overlay><template/></overlay>"#.to_vec());
    files.insert("themes/x.css".to_string(), css.clone());
    let mut m = minimal_manifest(&files, 1);
    let mut rk = BTreeMap::new();
    rk.insert("theme".to_string(), ResourceKind {
        dir: "themes".into(),
        extensions: vec!["css".into()],
        max_size_bytes: 2,
    });
    m.resource_kinds = Some(rk);
    let err = sanitize_bundle(&m, files).unwrap_err();
    assert!(matches!(err, SanitizeError::SizeExceeded { .. }));
}
