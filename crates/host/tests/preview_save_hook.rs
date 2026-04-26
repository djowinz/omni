//! Integration coverage for `share::save_preview` (upload-flow redesign §8.3).
//!
//! The full PNG-render path requires Ultralight resources next to the test
//! executable (same gate as `thumbnail_integration::generate_for_theme_smoke`),
//! so the end-to-end "writes valid PNG bytes" assertion lives behind
//! `#[ignore]` for the manual `cargo test -- --ignored` invocation. The
//! non-ignored tests cover everything reachable WITHOUT a live render
//! channel:
//!
//! - The public surface compiles + links (`use` lines below).
//! - Missing-file inputs surface a structured error (not a panic).
//! - When the live thumbnail channel is not installed, the parser/disk
//!   pipeline still runs and the failure surfaces as an Err — the save-hook
//!   integration site (downstream task) relies on this so it can swallow the
//!   error without the WS thread crashing.

use omni_host::share::save_preview::{
    render_overlay_preview, render_theme_preview, OVERLAY_PREVIEW_FILENAME, THEME_PREVIEW_SUFFIX,
};
use tempfile::tempdir;

// Type aliases keep the function-pointer signatures readable AND let clippy's
// `type_complexity` lint pass; the boxed-trait return is intentional (matches
// the public surface of `share::save_preview`).
type OverlayPreviewFn = fn(&std::path::Path) -> Result<(), Box<dyn std::error::Error>>;
type ThemePreviewFn = fn(&std::path::Path, &str) -> Result<(), Box<dyn std::error::Error>>;

/// Trivial "module loaded" check: confirms the public symbols exist with the
/// signatures the downstream save-hook integration will call. If any of the
/// referenced items go away, this binary fails to compile — surfacing the
/// breakage at `cargo test` rather than at integration time.
#[test]
fn public_surface_resolves() {
    let _o: OverlayPreviewFn = render_overlay_preview;
    let _t: ThemePreviewFn = render_theme_preview;
    assert!(OVERLAY_PREVIEW_FILENAME.starts_with('.'));
    assert!(THEME_PREVIEW_SUFFIX.ends_with(".png"));
}

#[test]
fn render_overlay_preview_errors_when_overlay_omni_missing() {
    let dir = tempdir().expect("tempdir");
    let overlay_dir = dir.path().join("overlays").join("empty-overlay");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir overlay_dir");
    let err = render_overlay_preview(&overlay_dir)
        .expect_err("missing overlay.omni must error, not panic");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(
        msg.contains("i/o") || msg.contains("io") || msg.contains("not found"),
        "expected I/O error mention, got: {msg}"
    );
}

#[test]
fn render_theme_preview_errors_when_css_missing() {
    let dir = tempdir().expect("tempdir");
    let themes_dir = dir.path().join("themes");
    std::fs::create_dir_all(&themes_dir).expect("mkdir themes");
    let err = render_theme_preview(&themes_dir, "missing.css")
        .expect_err("missing CSS must error, not panic");
    let msg = err.to_string().to_ascii_lowercase();
    assert!(
        msg.contains("i/o") || msg.contains("io") || msg.contains("not found"),
        "expected I/O error mention, got: {msg}"
    );
}

#[test]
fn render_overlay_preview_without_render_channel_returns_err() {
    // Without a live host (cargo test never starts the renderer), the
    // underlying `render_omni_to_png` returns
    // `ThumbnailError::RenderFailed { detail: "thumbnail channel not installed…" }`.
    // The save-hook caller relies on this graceful Err so a save can still
    // succeed; verify the function returns rather than panicking.
    let dir = tempdir().expect("tempdir");
    let overlay_dir = dir.path().join("overlays").join("test-overlay");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir overlay_dir");
    std::fs::write(
        overlay_dir.join("overlay.omni"),
        br#"<widget id="x" name="x" enabled="true">
  <template><div class="p"><span class="val">hi</span></div></template>
  <style>.p{color:#fff}</style>
</widget>"#,
    )
    .expect("write overlay");

    // Intentionally NOT asserting on which Err variant — the contract is
    // simply "no panic, returns Err". On a CI host with Ultralight installed
    // this may even succeed; either outcome is acceptable.
    let result = render_overlay_preview(&overlay_dir);
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(
            !msg.is_empty(),
            "preview error must carry a non-empty message"
        );
    }
}

/// End-to-end smoke — requires Ultralight resources next to the test
/// executable. Run manually with `cargo test -- --ignored`.
#[test]
#[ignore]
fn render_overlay_preview_writes_png() {
    let dir = tempdir().expect("tempdir");
    let overlay_dir = dir.path().join("overlays").join("smoke-overlay");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir overlay_dir");
    std::fs::write(
        overlay_dir.join("overlay.omni"),
        br#"<widget id="x" name="x" enabled="true">
  <template><div class="p"><span class="val">hi</span></div></template>
  <style>.p{color:#fff}</style>
</widget>"#,
    )
    .expect("write overlay");

    render_overlay_preview(&overlay_dir).expect("preview render failed");

    let preview = overlay_dir.join(OVERLAY_PREVIEW_FILENAME);
    assert!(
        preview.exists(),
        "expected preview at {}",
        preview.display()
    );
    let bytes = std::fs::read(&preview).expect("read preview");
    assert!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "expected PNG magic bytes"
    );
}
