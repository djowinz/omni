//! End-to-end integration test for the `pack_only` pipeline (Task A2.3 / OWI-43).
//!
//! Spec: docs/superpowers/specs/2026-04-21-upload-flow-redesign-design.md §10
//! done criterion #1 — "upload.pack end-to-end with a fixture overlay that
//! references a theme + font + image, assert the bundled artifact contains
//! all four files and the resolved manifest has correct `resource_kinds`."
//!
//! ## Scope
//!
//! This test wires together Wave A1's three independent guarantees that the
//! sibling tests pin in isolation:
//!
//! - `crates/host/tests/dep_resolver.rs` covers reference walking +
//!   missing/unused detection in `dep_resolver::resolve` against in-memory
//!   fixtures.
//! - `crates/host/tests/build_manifest_resource_kinds.rs` covers
//!   `build_manifest`'s `resource_kinds` population from file paths.
//! - This file drives the full `pack_only` entry point against a real
//!   tempdir workspace + real sanitize handlers + real signed-bundle
//!   serialization, then unpacks the resulting `.omnipkg` and asserts the
//!   shipped bytes match the four-file fixture.
//!
//! ## Fixture shape (deviation from spec text)
//!
//! The plan describes the fixture overlay as referencing the font via a
//! `<font src="fonts/d.ttf"/>` element. The shipped strict overlay
//! sanitizer (`crates/sanitize/src/handlers/overlay.rs`) only accepts
//! `<theme>`, `<config>`, and `<widget>` as top-level elements (see
//! `bundle::omni_schema::TOP_LEVEL_ELEMENTS`); a top-level `<font>` would
//! be rejected before pack ever sees it. The dep_resolver picks up
//! `<font src>` permissively, but the strict gate runs first.
//!
//! Per the prompt's "If the pack entry expects a different fixture shape,
//! adapt to what's shipped" instruction, the font reference is expressed via
//! a CSS `url(fonts/d.ttf)` value inside the inline `<style>` block. The
//! dep_resolver still picks it up (any non-`.css` `url()` value is a leaf
//! resource ref — see `dep_resolver::walk_theme`), it dispatches through
//! `FontHandler` because the path lives under `fonts/`, and the signed
//! bundle still ships the font. The four kinds (overlay, theme, font,
//! image) all land in `resource_kinds` exactly as the spec criterion
//! requires.
//!
//! ## Why this needs a fake thumbnail responder
//!
//! `pack_only` calls `render_thumbnail` (Ultralight off-screen render). The
//! shipped pipeline routes thumbnail requests through a process-wide channel
//! installed by `main.rs::install_thumbnail_channel` (architectural
//! invariant #24 — Ultralight has process-global state). Without an
//! installed channel, `render_omni_to_png` returns
//! `ThumbnailError::RenderFailed { detail: "thumbnail channel not
//! installed" }` and the pack call errors out before producing the bundle.
//!
//! Sibling integration tests in this crate
//! (`share_upload_integration::happy_path_upload_emits_jws_header_and_progress`
//! and `share::upload::tests::pack_only_theme_roundtrips`) take the
//! `#[ignore]` route and require running the host binary first to set the
//! channel. To make THIS test runnable under default `cargo test -p host
//! --test upload_pack_integration`, we install a fake channel inside the
//! test binary that responds with a 1×1 dummy BGRA frame. The downstream
//! letterbox + PNG-encode path in `share::thumbnail::mod` accepts any
//! width/height, so 1×1 is sufficient and no real Ultralight render
//! happens. The fixture overlay still has to be parser-valid because
//! `share::thumbnail::bundle::generate_for_bundle` runs
//! `parse_omni_with_diagnostics` on the entry overlay before dispatching
//! the render request — the fake channel only short-circuits the GPU/CPU
//! work, not the upstream parse.

use std::collections::BTreeMap;
use std::sync::Arc;

use bundle::{BundleLimits, ResourceKind};
use identity::unpack_signed_bundle;
use omni_host::share::upload::{pack_only, ArtifactKind, UploadRequest};
use omni_host::ul_renderer::{
    get_thumbnail_channel, install_thumbnail_channel, ThumbnailPixels, ThumbnailRequest,
};
use test_harness::deterministic_keypair;
use tokio::sync::mpsc;

const TEST_OVERLAY_OMNI: &[u8] = br#"<theme src="themes/m.css"/>
<widget id="t" name="Test" enabled="true">
<template><div>hello</div></template>
<style>.x{background:url(images/bg.png)} .y{background:url(fonts/d.ttf)}</style>
</widget>
"#;

const TEST_THEME_CSS: &[u8] = b":root{--bg:#000;--text:#fff}\n";

/// Real TTF fixture vendored in `crates/sanitize/tests/fixtures/font/`. The
/// font handler runs `ttf_parser::Face::parse` so anything synthesized at
/// test time would have to satisfy the full TrueType structural contract;
/// the easier path is to reuse the same fixture the sanitize crate already
/// uses to prove its handler accepts a real-world TTF.
const TEST_FONT_TTF: &[u8] = include_bytes!("../../sanitize/tests/fixtures/font/ok.ttf");

/// Install a process-wide thumbnail responder if one isn't already
/// installed. Returns true on the first install, false if a previous test
/// in the same binary already wired one up. The responder is intentionally
/// a "dummy 1×1 BGRA" fast path — we don't exercise Ultralight from this
/// test binary, only the file-flow through `pack_only`.
///
/// `install_thumbnail_channel` is backed by `OnceLock`; the second + later
/// callers in the same process are silently ignored. Inside a single
/// `#[test]` the install is unconditional, but if this file ever grows to
/// multiple tests they'll share the same responder.
fn install_dummy_thumbnail_responder() {
    if get_thumbnail_channel().is_some() {
        return;
    }
    let (tx, mut rx) = mpsc::unbounded_channel::<ThumbnailRequest>();
    install_thumbnail_channel(tx);

    // Responder lives for the duration of the tokio runtime — i.e. until
    // the `#[tokio::test]`'s async block returns. The OnceLock holds the
    // sender; once the runtime drops, sends will fail, but by then no more
    // pack_only calls are in flight.
    tokio::spawn(async move {
        while let Some(req) = rx.recv().await {
            // Single transparent BGRA pixel: B=0, G=0, R=0, A=0.
            let pixels = ThumbnailPixels {
                width: 1,
                height: 1,
                row_bytes: 4,
                bgra: vec![0, 0, 0, 0],
                widget_bbox: None,
            };
            let _ = req.reply.send(Ok(pixels));
        }
    });
}

#[tokio::test]
async fn pack_only_bundles_overlay_theme_font_and_image() {
    install_dummy_thumbnail_responder();

    // ── Fixture workspace layout ─────────────────────────────────────────
    // Per shipped invariant #13: overlays live at `<workspace>/<name>/` with
    // sibling `themes/`, `fonts/`, `images/` subfolders. `walk_bundle`
    // (`share::upload::walk_bundle`) walks the workspace path passed as
    // `source_path`, so the tempdir IS the overlay's root directory.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("overlay.omni"), TEST_OVERLAY_OMNI).expect("write overlay.omni");
    std::fs::create_dir_all(root.join("themes")).expect("mkdir themes");
    std::fs::write(root.join("themes/m.css"), TEST_THEME_CSS).expect("write theme");
    std::fs::create_dir_all(root.join("fonts")).expect("mkdir fonts");
    std::fs::write(root.join("fonts/d.ttf"), TEST_FONT_TTF).expect("write font");
    std::fs::create_dir_all(root.join("images")).expect("mkdir images");
    std::fs::write(root.join("images/bg.png"), tiny_png()).expect("write image");

    // ── Drive the pack pipeline ──────────────────────────────────────────
    let kp = Arc::new(deterministic_keypair());
    let req = UploadRequest {
        kind: ArtifactKind::Bundle,
        source_path: root.to_path_buf(),
        name: "test-overlay".into(),
        description: "integration fixture".into(),
        tags: vec![],
        license: "MIT".into(),
        version: "1.0.0".parse().unwrap(),
        omni_min_version: "0.1.0".parse().unwrap(),
        update_artifact_id: None,
    };
    let pack = pack_only(&req, &BundleLimits::DEFAULT, &kp)
        .await
        .expect("pack_only must succeed for valid four-file overlay fixture");

    // ── Manifest resource_kinds (spec §8.7 / OWI-33) ────────────────────
    // Every kind the bundle ships must be declared so the Worker's
    // `isThemeOnly()` correctly classifies the upload as a bundle, not a
    // theme. Build_manifest's path-to-kind mapping is itself unit-tested in
    // `build_manifest_resource_kinds.rs`; here we verify the same map
    // survives the sanitize → sign → unpack roundtrip and arrives intact in
    // the bytes that ship to the Worker.
    let kinds: &BTreeMap<String, ResourceKind> = pack
        .manifest
        .resource_kinds
        .as_ref()
        .expect("resource_kinds must be populated post-pack");
    assert!(
        kinds.contains_key("overlay"),
        "missing overlay kind; got {:?}",
        kinds.keys().collect::<Vec<_>>()
    );
    assert!(
        kinds.contains_key("theme"),
        "missing theme kind; got {:?}",
        kinds.keys().collect::<Vec<_>>()
    );
    assert!(
        kinds.contains_key("font"),
        "missing font kind; got {:?}",
        kinds.keys().collect::<Vec<_>>()
    );
    assert!(
        kinds.contains_key("image"),
        "missing image kind; got {:?}",
        kinds.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        kinds.len(),
        4,
        "exactly four kinds expected (overlay, theme, font, image); got {:?}",
        kinds.keys().collect::<Vec<_>>()
    );

    // ── Bundle bytes contain all four files ──────────────────────────────
    // `pack.sanitized_bytes` is the signed `.omnipkg` envelope. Round-trip
    // through `identity::unpack_signed_bundle` verifies the JWS, the
    // canonical hash, AND surfaces the file map for content assertions —
    // catching any silent loss between `walk_bundle` and `pack_signed_bundle`.
    let signed = unpack_signed_bundle(&pack.sanitized_bytes, None, &BundleLimits::DEFAULT)
        .expect("signed bundle unpack must succeed");
    let (_unpacked_manifest, files) = signed.into_files_map();
    let bundled_paths: Vec<&str> = files.keys().map(String::as_str).collect();
    for required in [
        "overlay.omni",
        "themes/m.css",
        "fonts/d.ttf",
        "images/bg.png",
    ] {
        assert!(
            files.contains_key(required),
            "bundle missing required file {required:?}; got {bundled_paths:?}"
        );
    }
}

/// Smallest valid PNG: 1×1 transparent pixel. The image sanitize handler
/// (`crates/sanitize/src/handlers/image.rs`) decodes via the `image` crate
/// then re-encodes as PNG; magic-only bytes would fail the decode step. We
/// build the PNG with the same `image` crate the handler uses so format
/// drift can't desync the fixture.
fn tiny_png() -> Vec<u8> {
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_pixel(1, 1, Rgba([0, 0, 0, 0]));
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("encode tiny PNG fixture");
    buf
}
