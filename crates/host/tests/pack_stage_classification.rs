//! Integration test: `pack_only_with_progress` routes sanitize failures to
//! the correct Step 3 pack stage (OWI-89).
//!
//! Regression: before OWI-89, `sanitize_bundle` (one atomic call) failures
//! always landed on the `ContentSafety` row of the renderer's Step 3 UI —
//! including envelope / structural errors that semantically belong on
//! `Schema`. The user-visible symptom: putting an unsupported HTML element
//! at the top level of `overlay.omni` failed at "Content-Safety Checks"
//! when it should have failed at "Schema Validation."
//!
//! `classify_sanitize_error` (in `crates/host/src/share/upload.rs`) inspects
//! the `SanitizeError` post-hoc and routes the `Failed` frame to whichever
//! stage the error semantically belongs to:
//!
//! * Envelope / structural errors (overlay handler) → `Schema`
//! * URL / scheme / `@import` violations (theme + overlay handlers) →
//!   `ContentSafety`
//! * Image / font decode + magic failures → `Asset`
//! * Non-`Handler` SanitizeError variants (Malformed / executable-magic /
//!   unknown-kind / size-exceeded) → `Schema`
//!
//! Each test case below builds a minimal on-disk fixture, runs the
//! production `pack_only_with_progress` with a channel-backed
//! `PackProgressSink`, and asserts the LAST `Failed` frame the renderer
//! would observe lands on the expected stage. The renderer's
//! `use-pack-progress` hook accumulates by stage with last-write-wins
//! semantics, so a `Schema: Failed` frame after an earlier `Schema: Passed`
//! re-routes the failure UI to the schema row — the
//! `unsupported_element_in_overlay_classifies_as_schema` case below locks
//! that behavior in.
//!
//! These tests do NOT need Ultralight resources: every failure path returns
//! BEFORE the `render_thumbnail` step, so they run on every
//! `cargo test -p host` invocation (no `#[ignore]` gate).

use std::path::Path;

use bundle::BundleLimits;
use omni_host::share::upload::{
    pack_only_with_progress, ArtifactKind, PackProgressSink, UploadRequest,
};
use omni_host::share::ws_messages::{PackProgress, PackStage, StageStatus};
use test_harness::deterministic_keypair;
use tokio::sync::mpsc;

/// A minimal valid overlay. Used as the "innocent bystander" in tests that
/// inject a single bad asset (font / image) — the asset is what should
/// fail, not the overlay envelope. Matches the shape used elsewhere in
/// `crates/host/tests/upload_pack_orphan_image.rs`.
const VALID_OVERLAY: &[u8] = b"<widget><template><div/></template></widget>";

/// Truncated PNG: just the 8-byte PNG signature, no IHDR / IDAT / IEND.
/// Passes the executable-magic deny-list (PNG signature isn't on the list)
/// and reaches `ImageHandler::sanitize`, where
/// `image::load_from_memory_with_format` errors with "decode: ...".
fn truncated_png() -> Vec<u8> {
    vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
}

/// 8 bytes that are NOT a valid TTF / OTF / WOFF2 magic. The font handler
/// rejects these with "bad magic [..]" before ttf-parser gets a chance to
/// run. `0xDE 0xAD 0xBE 0xEF` is unambiguously not in the allowed magic
/// set ([0x00, 0x01, 0x00, 0x00] | "OTTO" | "wOF2" | "true" | "typ1").
fn bad_ttf_bytes() -> Vec<u8> {
    vec![0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00]
}

/// Channel-backed [`PackProgressSink`] that captures every emitted frame.
/// Wraps an `mpsc::UnboundedSender` so the production pipeline can call
/// `try_send` (the bounded sender adapter would drop frames at capacity;
/// we want every frame retained).
struct CapturingSink {
    tx: mpsc::UnboundedSender<PackProgress>,
}

impl PackProgressSink for CapturingSink {
    fn emit(&self, frame: PackProgress) {
        let _ = self.tx.send(frame);
    }
}

/// Drive `pack_only_with_progress` against an on-disk fixture and return
/// every `PackProgress` frame the production pipeline emitted, in order.
async fn run_and_capture(req: UploadRequest) -> Vec<PackProgress> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let sink = CapturingSink { tx };
    let identity = deterministic_keypair();
    let _ = pack_only_with_progress(&req, &BundleLimits::DEFAULT, &identity, Some(&sink)).await;
    drop(sink); // close the sender so the receiver drain terminates.
    let mut frames = Vec::new();
    while let Some(frame) = rx.recv().await {
        frames.push(frame);
    }
    frames
}

/// Find the LAST `Failed` frame in the captured stream — this is the frame
/// the renderer's `use-pack-progress` hook surfaces in the Step 3 UI
/// (last-write-wins per stage means the renderer paints whatever stage
/// most recently flipped to Failed).
fn last_failed(frames: &[PackProgress]) -> &PackProgress {
    frames
        .iter()
        .rev()
        .find(|f| f.status == StageStatus::Failed)
        .unwrap_or_else(|| {
            panic!(
                "expected at least one Failed frame in the pack progress stream; \
                 captured frames = {frames:#?}"
            )
        })
}

fn bundle_request(workspace: &Path, name: &str) -> UploadRequest {
    UploadRequest {
        kind: ArtifactKind::Bundle,
        source_path: workspace.to_path_buf(),
        name: name.into(),
        description: "OWI-89 stage-classification fixture".into(),
        tags: vec![],
        license: "MIT".into(),
        version: "1.0.0".parse().expect("semver"),
        omni_min_version: "0.1.0".parse().expect("semver"),
        update_artifact_id: None,
    }
}

fn theme_request(css_path: &Path, name: &str) -> UploadRequest {
    UploadRequest {
        kind: ArtifactKind::Theme,
        source_path: css_path.to_path_buf(),
        name: name.into(),
        description: "OWI-89 stage-classification fixture".into(),
        tags: vec![],
        license: "MIT".into(),
        version: "1.0.0".parse().expect("semver"),
        omni_min_version: "0.1.0".parse().expect("semver"),
        update_artifact_id: None,
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Test cases (mirror the table in OWI-89's "Owns" section).
// ─────────────────────────────────────────────────────────────────────────

/// `<garbage_element/>` at the top level of the overlay → overlay handler's
/// `validate_envelope_empty` returns `"unexpected top-level
/// <garbage_element/>"`. Detail contains "unexpected", "top-level",
/// "element" → classifier picks `Schema`.
///
/// This is the user-visible symptom from the OWI-89 bug report: pre-fix,
/// this case landed on `ContentSafety` because the entire sanitize call
/// was attributed to that stage by default.
#[tokio::test]
async fn unsupported_element_in_overlay_classifies_as_schema() {
    let workspace = tempfile::tempdir().expect("tempdir");
    // Valid widget envelope so we exercise validate_structure's depth-1
    // walk; the second top-level element is the structural reject.
    std::fs::write(
        workspace.path().join("overlay.omni"),
        b"<widget><template><div/></template></widget><garbage_element/>",
    )
    .expect("write overlay");

    let frames = run_and_capture(bundle_request(workspace.path(), "schema-garbage")).await;
    let failed = last_failed(&frames);
    assert_eq!(
        failed.stage,
        PackStage::Schema,
        "unsupported top-level element must surface on the Schema row, not \
         ContentSafety (OWI-89 regression). Captured frames = {frames:#?}"
    );

    // Sanity: the renderer relies on Schema previously emitting `Passed`
    // (build_manifest succeeded) and then `Failed` after sanitize. Confirm
    // both frames are present so the last-write-wins assumption documented
    // in `classify_sanitize_error`'s emit_sanitize_failure helper holds.
    let schema_frames: Vec<&PackProgress> = frames
        .iter()
        .filter(|f| f.stage == PackStage::Schema)
        .collect();
    assert!(
        schema_frames
            .iter()
            .any(|f| f.status == StageStatus::Passed),
        "Schema must emit `Passed` first (build_manifest succeeded) before \
         the post-sanitize `Failed` re-routes the UI; got {schema_frames:#?}"
    );
    assert!(
        schema_frames
            .iter()
            .any(|f| f.status == StageStatus::Failed),
        "Schema must emit `Failed` after the classifier reroutes; got \
         {schema_frames:#?}"
    );
}

/// `<style>@import url(x.css)</style>` inside a valid `<widget>` envelope →
/// `theme::sanitize_css` rejects with `"@import disallowed"` (kind is
/// "overlay" because the call site is the overlay handler delegating to
/// the shared CSS sanitizer). Detail does not match any schema substring →
/// classifier picks `ContentSafety`.
#[tokio::test]
async fn at_import_in_overlay_style_classifies_as_content_safety() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        workspace.path().join("overlay.omni"),
        b"<widget><style>@import url(\"x.css\");</style></widget>",
    )
    .expect("write overlay");

    let frames = run_and_capture(bundle_request(workspace.path(), "cs-import")).await;
    let failed = last_failed(&frames);
    assert_eq!(
        failed.stage,
        PackStage::ContentSafety,
        "@import inside an overlay <style> is a content-safety failure, not \
         schema. Captured frames = {frames:#?}"
    );
}

/// `javascript:` URL inside a `<style>` body → `sanitize_css::validate_url`
/// rejects with `"disallowed scheme in url(): javascript:..."`. Detail
/// contains "scheme" but no schema substring → `ContentSafety`.
#[tokio::test]
async fn javascript_url_in_overlay_style_classifies_as_content_safety() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        workspace.path().join("overlay.omni"),
        b"<widget><style>.x { background: url(javascript:alert(1)); }</style></widget>",
    )
    .expect("write overlay");

    let frames = run_and_capture(bundle_request(workspace.path(), "cs-jsurl")).await;
    let failed = last_failed(&frames);
    assert_eq!(
        failed.stage,
        PackStage::ContentSafety,
        "javascript: scheme inside url() is content-safety, not asset/schema. \
         Captured frames = {frames:#?}"
    );
}

/// Same scheme rejection but exercised through the standalone theme path
/// (`ArtifactKind::Theme` → `sanitize_theme`, kind="theme") to confirm the
/// classifier handles both kinds identically.
#[tokio::test]
async fn at_import_in_theme_classifies_as_content_safety() {
    let tmp = tempfile::NamedTempFile::new().expect("tmp");
    std::fs::write(tmp.path(), b"@import url(\"x.css\");\n:root { --x: 1; }")
        .expect("write theme css");

    let frames = run_and_capture(theme_request(tmp.path(), "cs-theme-import")).await;
    let failed = last_failed(&frames);
    assert_eq!(
        failed.stage,
        PackStage::ContentSafety,
        "@import in a standalone theme.css is content-safety. \
         Captured frames = {frames:#?}"
    );
}

/// Truncated PNG bytes under `images/` → image handler's `decode: ...`
/// error → kind="image" → `Asset`.
#[tokio::test]
async fn truncated_png_classifies_as_asset() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("overlay.omni"), VALID_OVERLAY)
        .expect("write overlay");
    let images = workspace.path().join("images");
    std::fs::create_dir(&images).expect("mkdir images");
    std::fs::write(images.join("bad.png"), truncated_png()).expect("write truncated png");

    let frames = run_and_capture(bundle_request(workspace.path(), "asset-png")).await;
    let failed = last_failed(&frames);
    assert_eq!(
        failed.stage,
        PackStage::Asset,
        "truncated PNG must surface on the Asset row (image decode \
         failure). Captured frames = {frames:#?}"
    );
}

/// Bad TTF magic bytes under `fonts/` → font handler's `bad magic [..]`
/// error → kind="font" → `Asset`.
#[tokio::test]
async fn bad_ttf_magic_classifies_as_asset() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(workspace.path().join("overlay.omni"), VALID_OVERLAY)
        .expect("write overlay");
    let fonts = workspace.path().join("fonts");
    std::fs::create_dir(&fonts).expect("mkdir fonts");
    std::fs::write(fonts.join("bad.ttf"), bad_ttf_bytes()).expect("write bad ttf");

    let frames = run_and_capture(bundle_request(workspace.path(), "asset-ttf")).await;
    let failed = last_failed(&frames);
    assert_eq!(
        failed.stage,
        PackStage::Asset,
        "bad TTF magic must surface on the Asset row (font handler reject). \
         Captured frames = {frames:#?}"
    );
}

/// DOCTYPE in overlay → overlay handler returns `"DOCTYPE disallowed"` →
/// `Schema`. Locks in the second canonical structural-rejection path
/// alongside the unsupported-element case so a future tweak that
/// accidentally rewords the detail to lose the "doctype" substring fails
/// loud.
#[tokio::test]
async fn doctype_in_overlay_classifies_as_schema() {
    let workspace = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        workspace.path().join("overlay.omni"),
        b"<!DOCTYPE x><widget><template><div/></template></widget>",
    )
    .expect("write overlay");

    let frames = run_and_capture(bundle_request(workspace.path(), "schema-doctype")).await;
    let failed = last_failed(&frames);
    assert_eq!(
        failed.stage,
        PackStage::Schema,
        "DOCTYPE in overlay is a structural / schema reject, not \
         content-safety. Captured frames = {frames:#?}"
    );
}

/// Non-`Handler` SanitizeError (executable magic on a CSS file) →
/// `Schema`. Confirms the catch-all branch in `classify_sanitize_error`
/// for pre-handler-dispatch errors.
#[tokio::test]
async fn executable_magic_in_theme_classifies_as_schema() {
    let tmp = tempfile::NamedTempFile::new().expect("tmp");
    // MZ signature (Windows PE) — first entry in the executable-magic
    // deny-list. `sanitize_theme` rejects with
    // `SanitizeError::RejectedExecutableMagic`, which the classifier maps
    // to `Schema` (non-Handler variant catch-all).
    std::fs::write(tmp.path(), [0x4Du8, 0x5A, 0x00, 0x00, 0x00, 0x00])
        .expect("write fake exe css");

    let frames = run_and_capture(theme_request(tmp.path(), "schema-exe-magic")).await;
    let failed = last_failed(&frames);
    assert_eq!(
        failed.stage,
        PackStage::Schema,
        "executable-magic rejection is a non-Handler SanitizeError variant \
         and must classify as Schema (pre-handler-dispatch). Captured frames \
         = {frames:#?}"
    );
}
