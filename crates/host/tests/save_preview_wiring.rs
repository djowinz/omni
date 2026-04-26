//! Integration coverage for the `share::save_preview` wiring inside
//! `workspace::file_api::handle_write` (upload-flow redesign §8.3, Task A2.0).
//!
//! Spec: §8.3 (`.omni-preview.png` written by overlay-save and theme-save host
//! paths) + §7.1.9 (source picker reads the preview file). The actual PNG-byte
//! assertion requires Ultralight resources next to the test executable (same
//! gate as `preview_save_hook::render_overlay_preview_writes_png`), so the
//! end-to-end "preview file appears on disk" assertion lives behind
//! `#[ignore]`. The non-ignored tests cover the wiring contract WITHOUT a live
//! render channel:
//!
//! - `handle_write` returns `file.written` (never `error`) for overlay.omni
//!   and theme `.css` saves, even when the preview render fails because no
//!   live render channel is installed (the hook MUST swallow failures per
//!   spec §8.3 and `notes/0.1-save-hook-location.md`).
//! - Saving an unrelated path (random `.json`, asset, etc.) produces no
//!   preview-shaped sibling file — verifies the path classifier does NOT
//!   misroute non-overlay / non-theme writes.
//! - Saving a CSS file OUTSIDE `themes/` (e.g. inside an overlay folder)
//!   produces no theme-preview sibling — verifies the `themes/` prefix gate.

use omni_host::share::save_preview::{OVERLAY_PREVIEW_FILENAME, THEME_PREVIEW_SUFFIX};
use omni_host::workspace::file_api::handle_write;
use std::fs;
use tempfile::tempdir;

/// Minimal valid overlay XML the parser accepts. Identical shape to the
/// fixture used by `preview_save_hook::render_overlay_preview_without_render_channel_returns_err`
/// so both binaries exercise the same parser path.
const OVERLAY_XML: &str = r#"<widget id="x" name="x" enabled="true">
  <template><div class="p"><span class="val">hi</span></div></template>
  <style>.p{color:#fff}</style>
</widget>"#;

const THEME_CSS: &str = ":root { --accent: #00d9ff; }\n";

fn make_data_dir() -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    fs::create_dir_all(dir.path().join("overlays")).expect("mkdir overlays");
    fs::create_dir_all(dir.path().join("themes")).expect("mkdir themes");
    dir
}

#[test]
fn overlay_save_succeeds_even_when_preview_render_fails() {
    // Without a live render thread (cargo test never starts the host renderer),
    // `render_overlay_preview` returns an Err. The save hook MUST swallow that
    // error and still return `file.written` to the renderer — otherwise every
    // overlay save would surface a spurious WS error.
    let dir = make_data_dir();
    let overlay_dir = dir.path().join("overlays").join("test-overlay");
    fs::create_dir_all(&overlay_dir).expect("mkdir overlay dir");

    let result = handle_write(
        dir.path(),
        "overlays/test-overlay/overlay.omni",
        OVERLAY_XML,
    );

    assert_eq!(
        result["type"], "file.written",
        "save must succeed even when preview render fails: {result}",
    );
    assert_eq!(
        result["path"], "overlays/test-overlay/overlay.omni",
        "response path must echo the requested relative_path: {result}",
    );

    // The source overlay.omni must be on disk (the actual save did happen).
    assert!(
        overlay_dir.join("overlay.omni").exists(),
        "overlay.omni must exist after save",
    );
}

#[test]
fn theme_save_succeeds_even_when_preview_render_fails() {
    let dir = make_data_dir();

    let result = handle_write(dir.path(), "themes/dark.css", THEME_CSS);

    assert_eq!(
        result["type"], "file.written",
        "theme save must succeed even when preview render fails: {result}",
    );
    assert_eq!(
        result["path"], "themes/dark.css",
        "response path must echo the requested relative_path: {result}",
    );
    assert!(
        dir.path().join("themes/dark.css").exists(),
        "theme CSS must exist after save",
    );
}

#[test]
fn unrelated_path_save_does_not_attempt_any_preview() {
    // A random `.json` write under `overlays/` should NOT trigger either preview
    // render. We verify side-effect-free behaviour by checking that no preview
    // file (overlay dotfile or theme `.preview.png` sibling) appears anywhere
    // beneath the data dir.
    let dir = make_data_dir();
    let overlay_dir = dir.path().join("overlays").join("test-overlay");
    fs::create_dir_all(&overlay_dir).expect("mkdir overlay dir");

    let result = handle_write(
        dir.path(),
        "overlays/test-overlay/some-asset.json",
        r#"{"hello":"world"}"#,
    );
    assert_eq!(result["type"], "file.written");

    // Source file present; no preview shapes anywhere.
    assert!(overlay_dir.join("some-asset.json").exists());
    assert!(
        !overlay_dir.join(OVERLAY_PREVIEW_FILENAME).exists(),
        "no overlay preview sibling for unrelated writes",
    );
    assert!(
        !preview_files_present(dir.path()),
        "no preview-shaped file may exist anywhere after an unrelated save",
    );
}

#[test]
fn css_outside_themes_does_not_trigger_theme_preview() {
    // The classifier requires `themes/` prefix; saving a `.css` inside an
    // overlay folder must NOT go through the theme-preview path.
    let dir = make_data_dir();
    let overlay_dir = dir.path().join("overlays").join("test-overlay");
    fs::create_dir_all(&overlay_dir).expect("mkdir overlay dir");

    let result = handle_write(
        dir.path(),
        "overlays/test-overlay/extra.css",
        ":root {}",
    );
    assert_eq!(result["type"], "file.written");

    // No theme-preview sibling under themes/, no overlay-preview dotfile under
    // the overlay folder.
    let stray_theme_preview = dir.path().join("themes").join(format!(
        "extra.css{THEME_PREVIEW_SUFFIX}",
    ));
    assert!(
        !stray_theme_preview.exists(),
        "writing a .css file outside themes/ must not write a theme preview",
    );
    assert!(
        !overlay_dir.join(OVERLAY_PREVIEW_FILENAME).exists(),
        "writing a .css file is not an overlay save and must not write the overlay dotfile",
    );
}

#[test]
fn overlay_save_path_with_invalid_traversal_is_rejected_before_hook() {
    // The path validator runs BEFORE the write (and therefore before the
    // preview hook). Confirm the existing path-traversal guard still produces
    // an error response shape, with no preview side-effect on disk.
    let dir = make_data_dir();
    let result = handle_write(
        dir.path(),
        "../escape/overlay.omni",
        OVERLAY_XML,
    );
    assert_eq!(result["type"], "error");
    assert!(!preview_files_present(dir.path()));
}

/// End-to-end smoke — requires Ultralight resources next to the test
/// executable. Confirms the save-hook actually invokes the renderer pipeline
/// and writes the dotfile alongside the overlay. Run with
/// `cargo test -p host --test save_preview_wiring -- --ignored`.
#[test]
#[ignore]
fn overlay_save_writes_preview_dotfile_when_render_succeeds() {
    let dir = make_data_dir();
    let overlay_dir = dir.path().join("overlays").join("smoke-overlay");
    fs::create_dir_all(&overlay_dir).expect("mkdir overlay dir");

    let result = handle_write(
        dir.path(),
        "overlays/smoke-overlay/overlay.omni",
        OVERLAY_XML,
    );
    assert_eq!(result["type"], "file.written");

    let preview = overlay_dir.join(OVERLAY_PREVIEW_FILENAME);
    assert!(
        preview.exists(),
        "expected preview at {}",
        preview.display(),
    );
    let bytes = fs::read(&preview).expect("read preview");
    assert!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "expected PNG magic bytes",
    );
}

/// Walk the data dir and look for any file matching the preview-shape filename
/// patterns from `share::save_preview`. Used to assert that classifier-skipped
/// paths do not produce side effects.
fn preview_files_present(root: &std::path::Path) -> bool {
    fn walk(dir: &std::path::Path) -> bool {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return false,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if walk(&path) {
                    return true;
                }
                continue;
            }
            if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                if name == OVERLAY_PREVIEW_FILENAME {
                    return true;
                }
                if name.ends_with(THEME_PREVIEW_SUFFIX) {
                    return true;
                }
            }
        }
        false
    }
    walk(root)
}
