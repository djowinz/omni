//! Regression tests for `build_manifest`'s `resource_kinds` population
//! (spec §8.7 / OWI-33).
//!
//! Before this fix, `build_manifest` always shipped `resource_kinds: None`,
//! and the Worker's `isThemeOnly()` defaulted absent maps to `true` —
//! biasing every uploaded bundle into the theme bucket. This test pins the
//! mapping from bundle file paths to the kind names declared by the shipped
//! sanitize handlers (`theme`, `font`, `image`, `overlay`). The Worker's
//! `isThemeOnly()` (`apps/worker/src/routes/upload.ts`) only inspects the
//! map's keys, so as long as those keys are correct the routing works.
//!
//! Note: the Rust `Manifest.resource_kinds` field is
//! `Option<BTreeMap<String, bundle::ResourceKind>>`, NOT a count map. Each
//! value is a `ResourceKind { dir, extensions, max_size_bytes }` declaration
//! whose contents mirror the shipped sanitize handler defaults so the
//! bundle still routes through the correct handler in
//! `omni_sanitize::handlers::dispatch_for_path`.

use std::collections::BTreeMap;

use omni_host::share::upload::{build_manifest_for_test, ArtifactKind, UploadRequest};

fn test_request(kind: ArtifactKind) -> UploadRequest {
    UploadRequest {
        kind,
        // `source_path` is unused by `build_manifest` — only the metadata fields
        // it copies into the manifest matter here.
        source_path: std::path::PathBuf::from("/tmp/unused"),
        name: "fixture".into(),
        description: "test".into(),
        tags: vec![],
        license: "MIT".into(),
        version: "1.0.0".parse().unwrap(),
        omni_min_version: "0.1.0".parse().unwrap(),
        update_artifact_id: None,
    }
}

#[test]
fn populates_resource_kinds_for_overlay_bundle() {
    let mut files = BTreeMap::new();
    files.insert(
        "overlay.omni".into(),
        b"<widget><template><div/></template></widget>".to_vec(),
    );
    files.insert("theme.css".into(), b".x{color:red}".to_vec());
    files.insert("fonts/sans.ttf".into(), vec![0, 0, 0]);
    files.insert("images/logo.png".into(), vec![0, 0, 0]);

    let req = test_request(ArtifactKind::Bundle);
    let manifest = build_manifest_for_test(&req, &files).expect("build_manifest");

    let kinds = manifest
        .resource_kinds
        .as_ref()
        .expect("resource_kinds populated");
    assert!(
        kinds.contains_key("overlay"),
        "missing overlay; got {kinds:?}"
    );
    assert!(kinds.contains_key("theme"), "missing theme; got {kinds:?}");
    assert!(kinds.contains_key("font"), "missing font; got {kinds:?}");
    assert!(kinds.contains_key("image"), "missing image; got {kinds:?}");
    assert_eq!(kinds.len(), 4, "exactly four kinds expected; got {kinds:?}");

    // Per-declaration shapes match the shipped sanitize handler defaults so
    // dispatch keeps working through `omni_sanitize::handlers::dispatch_for_path`.
    let theme = &kinds["theme"];
    assert_eq!(theme.dir, "themes");
    assert!(theme.extensions.iter().any(|e| e == "css"));
    let font = &kinds["font"];
    assert_eq!(font.dir, "fonts");
    assert!(font.extensions.iter().any(|e| e == "ttf"));
    let image = &kinds["image"];
    assert_eq!(image.dir, "images");
    assert!(image.extensions.iter().any(|e| e == "png"));
    let overlay = &kinds["overlay"];
    assert!(overlay.extensions.iter().any(|e| e == "omni"));
}

#[test]
fn populates_theme_only_when_only_css() {
    let mut files = BTreeMap::new();
    files.insert("theme.css".into(), b".x{color:red}".to_vec());

    let req = test_request(ArtifactKind::Theme);
    let manifest = build_manifest_for_test(&req, &files).expect("build_manifest");

    let kinds = manifest
        .resource_kinds
        .as_ref()
        .expect("resource_kinds populated");
    assert_eq!(kinds.len(), 1, "exactly one kind expected; got {kinds:?}");
    assert!(kinds.contains_key("theme"));
}
