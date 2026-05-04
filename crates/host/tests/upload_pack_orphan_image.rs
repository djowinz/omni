//! Integration test: `upload::pack_only` rejects an overlay bundle that
//! carries an unreferenced image under `images/` (OWI-44 / Task A2.4).
//!
//! Spec: `docs/superpowers/specs/2026-04-21-upload-flow-redesign-design.md`
//! §10 done criterion #2 — "upload.pack with orphan image under images/ →
//! UnusedFile violation in structured error".
//!
//! This test exercises the full pack pipeline (walk_bundle → build_manifest →
//! sanitize_bundle → dep_resolver) end-to-end against a real on-disk
//! workspace, not just the resolver in isolation. The unit-level resolver
//! coverage already lives in `crates/host/tests/dep_resolver.rs`; this test
//! verifies that the resolver's `Violation::UnusedFile` actually surfaces
//! through the production `pack_only` entry point as
//! `UploadError::DependencyViolations { violations: [..] }` with the
//! expected wire-shape (`kind = "unused-file"`, `path = "images/orphan.png"`).
//!
//! The orphan image is a real PNG (1×1, 4-channel) so it survives
//! `sanitize_bundle` (which decodes + re-encodes every image — fake magic
//! bytes would error out at the ContentSafety stage instead of reaching
//! Dependency). The overlay XML deliberately references nothing under
//! `images/` so the dep resolver flags the file as an orphan.
//!
//! `pack_only` returns `Err(DependencyViolations)` BEFORE the thumbnail
//! render stage, so this test does NOT need Ultralight resources and runs
//! on every `cargo test -p host` invocation (no `#[ignore]` gate).

use std::collections::BTreeMap;
use std::path::Path;

use bundle::BundleLimits;
use omni_host::share::error::UploadError;
use omni_host::share::upload::{pack_only, ArtifactKind, UploadRequest};
use test_harness::deterministic_keypair;

/// Build a minimal, real PNG (1×1 opaque red pixel). The bytes have to be a
/// decodable PNG so that `sanitize_bundle`'s `ImageHandler::sanitize`
/// (which calls `image::load_from_memory_with_format` then re-encodes as
/// PNG) succeeds. A fake `[0x89, 0x50, 0x4E, 0x47]` header would fail
/// decode at the ContentSafety stage and the test would never reach the
/// Dependency stage we want to assert on.
fn tiny_red_png() -> Vec<u8> {
    use image::{ImageBuffer, ImageOutputFormat, Rgba};
    let buf: ImageBuffer<Rgba<u8>, _> = ImageBuffer::from_pixel(1, 1, Rgba([255, 0, 0, 255]));
    let mut out = Vec::new();
    buf.write_to(&mut std::io::Cursor::new(&mut out), ImageOutputFormat::Png)
        .expect("encode tiny PNG");
    out
}

/// Write a minimal overlay.omni that has neither `<img>` tags nor inline
/// CSS `url(...)` values — nothing references `images/orphan.png`, so the
/// dep resolver will catalogue it as `Violation::UnusedFile`.
fn write_minimal_overlay(workspace: &Path) {
    let overlay_xml = b"<widget><template><div/></template></widget>";
    std::fs::write(workspace.join("overlay.omni"), overlay_xml).expect("write overlay.omni");
}

fn upload_request_for(workspace: &Path) -> UploadRequest {
    UploadRequest {
        kind: ArtifactKind::Bundle,
        source_path: workspace.to_path_buf(),
        name: "orphan-fixture".into(),
        description: "integration fixture for OWI-44".into(),
        tags: vec![],
        license: "MIT".into(),
        version: "1.0.0".parse().expect("semver"),
        omni_min_version: "0.1.0".parse().expect("semver"),
        update_artifact_id: None,
        custom_thumbnail_bytes: None,
    }
}

#[tokio::test]
async fn pack_only_rejects_orphan_image_with_unused_file_violation() {
    // 1. Build the on-disk workspace: an overlay that references nothing under
    //    images/ and an orphan PNG sitting in the same workspace root.
    let workspace = tempfile::tempdir().expect("tempdir");
    write_minimal_overlay(workspace.path());
    let images_dir = workspace.path().join("images");
    std::fs::create_dir(&images_dir).expect("create images/");
    std::fs::write(images_dir.join("orphan.png"), tiny_red_png()).expect("write orphan.png");

    // 2. Drive the production pack pipeline. `pack_only` runs
    //    walk_bundle → build_manifest → sanitize_bundle → dep_resolver and
    //    returns BEFORE any Ultralight thumbnail render on this failure path,
    //    so no graphics resources are required.
    let req = upload_request_for(workspace.path());
    let identity = deterministic_keypair();
    let err = pack_only(&req, &BundleLimits::DEFAULT, &identity)
        .await
        .expect_err("pack_only must reject a bundle carrying an orphan image");

    // 3. Assert the error variant + structured detail (spec §10 done #2 + the
    //    wire-shape contract codified in `error.rs::DependencyViolationDetail`).
    let violations = match err {
        UploadError::DependencyViolations { violations } => violations,
        other => panic!(
            "expected UploadError::DependencyViolations, got {other:?} (code={code})",
            code = other.code()
        ),
    };
    let by_path: BTreeMap<&str, &str> = violations
        .iter()
        .map(|v| (v.path.as_str(), v.kind.as_str()))
        .collect();
    let kind = by_path
        .get("images/orphan.png")
        .copied()
        .unwrap_or_else(|| {
            panic!("no violation for images/orphan.png; got violations: {violations:?}")
        });
    assert_eq!(
        kind, "unused-file",
        "orphan image must be tagged with the `unused-file` wire kind so the \
         renderer's PackingViolationsCard groups it under the unused-files \
         category (spec §7.8.4 / INV-7.8.5); got kind={kind:?} for the orphan \
         row in violations={violations:?}"
    );
}
