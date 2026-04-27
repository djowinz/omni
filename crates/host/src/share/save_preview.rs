//! Save-time `.omni-preview.png` / `<theme>.preview.png` rendering.
//!
//! Spec: upload-flow-redesign §8.3. Renders a thumbnail for the just-saved
//! overlay or theme and writes it alongside the source file so the upload
//! dialog's source picker (spec §7.1.9) can show real artwork instead of the
//! zinc placeholder. Wraps the existing
//! [`crate::share::thumbnail`] pipeline with workspace-file entry points so
//! the WS save-hook does not need to unpack a signed bundle to render a
//! preview.
//!
//! ## Naming
//!
//! Named `save_preview` (not `preview`) to avoid colliding with
//! [`crate::share::preview`], which is the in-session live theme-swap surface
//! (snapshot/apply/revert with auto-revert TTL — a different concept that
//! never touches disk).
//!
//! ## Wiring (deferred)
//!
//! This module exposes the two `render_*` entry points; the actual call site
//! inside `crates/host/src/workspace/file_api.rs::handle_write` is wired in a
//! downstream task per the Wave 0.1 location-finding report. The hook MUST
//! swallow render failures (log + continue) so a save acknowledgement is
//! never blocked by preview generation. See spec §8.3 last paragraph and the
//! 0.1 report's "Error handling" section for the exhaustive failure-mode
//! list.
//!
//! ## Concurrency
//!
//! Both entry points block on a `std::sync::mpsc::Receiver::recv` inside
//! [`crate::share::thumbnail::render_omni_to_png`] while the live render
//! loop services the request (~100 ms). The WS server runs the dispatch on a
//! dedicated `std::thread`, NOT a tokio task, so `spawn_blocking` is neither
//! needed nor available. See `notes/0.1-save-hook-location.md` for the full
//! analysis.

use std::path::Path;

use super::thumbnail;

/// Filename for overlay previews. Dotfile so `walk_bundle`'s dotfile filter
/// excludes it from upload bundles automatically (spec §8.3 last paragraph).
pub const OVERLAY_PREVIEW_FILENAME: &str = ".omni-preview.png";

/// Suffix appended to the theme CSS filename to derive the preview filename.
/// Themes are single files at `themes/<name>.css`; the preview lives next to
/// them at `themes/<name>.css.preview.png`. NOT a dotfile — `walk_bundle`
/// would pick this up if the theme upload path ever copied siblings, but the
/// theme-only upload path only sends the CSS bytes themselves, so this is
/// safe in shipped behavior. Flagged in the 0.1 report for an explicit
/// regression test in a downstream task.
pub const THEME_PREVIEW_SUFFIX: &str = ".preview.png";

/// Render and write the overlay preview PNG.
///
/// `overlay_dir` is the path to the overlay folder (e.g.
/// `<data_dir>/overlays/marathon-hud`). On success, writes the PNG bytes to
/// `<overlay_dir>/.omni-preview.png` and returns `Ok(())`. Errors propagate
/// from disk I/O, the parser, the thumbnail channel, or the encoder — see
/// [`crate::share::thumbnail::ThumbnailError`] for the full enumeration.
///
/// Boxed-trait return type is intentional: the save-hook caller swallows
/// every failure mode (per spec §8.3 / 0.1 report), so the public surface
/// is just "a thing that may fail." If a future caller needs richer
/// downcasting, surface the underlying [`crate::share::thumbnail::ThumbnailError`]
/// directly.
pub fn render_overlay_preview(overlay_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let png_bytes = thumbnail::bundle::generate_for_workspace_overlay(overlay_dir)?;
    std::fs::write(overlay_dir.join(OVERLAY_PREVIEW_FILENAME), png_bytes)?;
    Ok(())
}

/// Render and write the theme preview PNG.
///
/// `themes_dir` is the workspace `themes/` folder (e.g. `<data_dir>/themes`)
/// and `theme_filename` is the bare CSS filename (e.g. `dark.css`). On
/// success, writes the PNG bytes to
/// `<themes_dir>/<theme_filename>.preview.png` and returns `Ok(())`.
pub fn render_theme_preview(
    themes_dir: &Path,
    theme_filename: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let css_path = themes_dir.join(theme_filename);
    let png_bytes = thumbnail::theme::generate_for_workspace_theme(&css_path)?;
    let preview_name = format!("{theme_filename}{THEME_PREVIEW_SUFFIX}");
    std::fs::write(themes_dir.join(preview_name), png_bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_preview_filename_is_dotfile() {
        // Dotfile invariant: `walk_bundle`'s dotfile filter must exclude this
        // automatically (spec §8.3 last paragraph). If the constant is ever
        // edited to drop the leading '.', the upload bundle will start
        // shipping the preview PNG — break the test loud.
        assert!(OVERLAY_PREVIEW_FILENAME.starts_with('.'));
    }

    #[test]
    fn theme_preview_suffix_ends_with_png() {
        assert!(THEME_PREVIEW_SUFFIX.ends_with(".png"));
    }
}
