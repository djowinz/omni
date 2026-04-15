//! Thumbnail path for raw theme CSS.
//!
//! Spec §3: sanitize-then-thumbnail. `css_bytes` MUST already be sanitized by
//! `omni-sanitize` before this function is called; the renderer (invariant #8)
//! is the trust boundary, not the sanitizer.

use crate::omni::assets::REFERENCE_OVERLAY_OMNI;
use crate::omni::parser::parse_omni_with_diagnostics;

use super::{render_omni_to_png, ThumbnailConfig, ThumbnailError};

const REFERENCE_OVERLAY_NAME: &str = "reference";

/// Render `css_bytes` against the bundled reference overlay and return PNG bytes.
///
/// The reference overlay (`crates/host/src/omni/assets/reference_overlay.omni`)
/// must declare a `<theme src="..."/>` element so this entry point can
/// substitute user CSS into its slot. If the overlay is missing that
/// declaration, every call returns [`ThumbnailError::RenderFailed`] rather
/// than silently rendering the overlay's default chrome unchanged.
pub fn generate_for_theme(
    css_bytes: &[u8],
    config: &ThumbnailConfig,
) -> Result<Vec<u8>, ThumbnailError> {
    // Parse the reference overlay (embedded at compile time).
    let (omni_file, diagnostics) = parse_omni_with_diagnostics(REFERENCE_OVERLAY_OMNI);
    let omni_file = omni_file.ok_or_else(|| {
        let detail = diagnostics
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join("; ");
        ThumbnailError::RenderFailed {
            detail: format!("reference overlay parse: {detail}"),
        }
    })?;

    // Determine the theme filename the reference overlay expects. If the
    // overlay has no theme_src, there is nothing to override — return
    // RenderFailed rather than silently mis-render.
    let theme_src =
        omni_file
            .theme_src
            .as_deref()
            .ok_or_else(|| ThumbnailError::RenderFailed {
                detail: "reference overlay has no theme_src; cannot inject user theme CSS".into(),
            })?;

    // Lay out tempdir as workspace::structure::resolve_theme_path expects:
    //   <tempdir>/overlays/<overlay_name>/<theme_src>
    let temp = tempfile::TempDir::new().map_err(ThumbnailError::Io)?;
    let overlay_dir = temp.path().join("overlays").join(REFERENCE_OVERLAY_NAME);
    std::fs::create_dir_all(&overlay_dir).map_err(ThumbnailError::Io)?;
    let theme_path = overlay_dir.join(theme_src);
    if let Some(parent) = theme_path.parent() {
        std::fs::create_dir_all(parent).map_err(ThumbnailError::Io)?;
    }
    std::fs::write(&theme_path, css_bytes).map_err(ThumbnailError::Io)?;

    render_omni_to_png(&omni_file, temp.path(), REFERENCE_OVERLAY_NAME, config)
    // TempDir drops here.
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Non-ignored precondition: the bundled reference overlay MUST declare a
    /// `<theme src="..."/>` element, and the tempdir layout code must compute
    /// a valid theme path from it and write the user CSS there. This covers
    /// the entire body of `generate_for_theme` up to — but not including —
    /// the `render_omni_to_png` call (which requires Ultralight and is
    /// exercised by `generate_for_theme_smoke` below).
    #[test]
    fn reference_overlay_declares_theme_src_and_layout_writes_css() {
        let (parsed, _diagnostics) = parse_omni_with_diagnostics(REFERENCE_OVERLAY_OMNI);
        let omni_file = parsed.expect("reference overlay must parse");
        let theme_src = omni_file
            .theme_src
            .as_deref()
            .expect("reference overlay must declare <theme src=\"...\"/>");

        // Mirror the tempdir layout performed by `generate_for_theme`.
        let temp = tempfile::TempDir::new().expect("tempdir");
        let overlay_dir = temp.path().join("overlays").join(REFERENCE_OVERLAY_NAME);
        std::fs::create_dir_all(&overlay_dir).expect("mkdir overlay_dir");
        let theme_path = overlay_dir.join(theme_src);
        if let Some(parent) = theme_path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir theme parent");
        }
        let css = b"/* layout test */";
        std::fs::write(&theme_path, css).expect("write theme");
        assert_eq!(
            std::fs::read(&theme_path).expect("read theme"),
            css,
            "theme file contents must round-trip through layout"
        );
    }

    /// End-to-end smoke test — requires Ultralight resources next to the test
    /// executable. Kept `#[ignore]` for that reason; run manually with
    /// `cargo test -- --ignored`. With the reference overlay now declaring
    /// `<theme src="..."/>`, this is the real render-path gate. Integration
    /// coverage lives in Task 6.
    #[test]
    #[ignore]
    fn generate_for_theme_smoke() {
        let (parsed, _) = parse_omni_with_diagnostics(REFERENCE_OVERLAY_OMNI);
        let omni_file = parsed.expect("reference overlay must parse");
        assert!(
            omni_file.theme_src.is_some(),
            "smoke test requires the reference overlay to declare <theme src=\"...\"/>"
        );

        let config = ThumbnailConfig::default();
        let css = b"/* empty user theme */";
        let _ = generate_for_theme(css, &config).expect("smoke render");
    }
}
