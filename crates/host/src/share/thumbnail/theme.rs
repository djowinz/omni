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

    /// End-to-end smoke test — requires Ultralight resources next to the test
    /// executable and, currently, a `<theme src="..."/>` declaration in the
    /// bundled reference overlay (absent as of W2T2). Run manually with
    /// `cargo test -- --ignored`. Real coverage lives in Task 6's integration
    /// tests.
    #[test]
    #[ignore]
    fn generate_for_theme_smoke() {
        let config = ThumbnailConfig::default();
        let css = b"/* empty user theme */";
        let _ = generate_for_theme(css, &config).expect("smoke render");
    }

    /// Under the current reference overlay (no `<theme>` element),
    /// `generate_for_theme` must return [`ThumbnailError::RenderFailed`]
    /// rather than silently rendering the default chrome. If a future Task 2
    /// amendment adds a `<theme>` element, this test should flip to the smoke
    /// path.
    #[test]
    fn generate_for_theme_errors_when_reference_overlay_has_no_theme_src() {
        // The parser may independently succeed or emit non-error diagnostics;
        // regardless, if no theme_src is declared we expect RenderFailed.
        let (parsed, _diagnostics) = parse_omni_with_diagnostics(REFERENCE_OVERLAY_OMNI);
        if let Some(ref f) = parsed {
            if f.theme_src.is_some() {
                // Reference overlay now declares theme_src — this test is
                // obsolete; skip without failing.
                return;
            }
        }

        let config = ThumbnailConfig::default();
        let err = generate_for_theme(b"", &config).expect_err("must fail without theme_src");
        match err {
            ThumbnailError::RenderFailed { detail } => {
                assert!(
                    detail.contains("theme_src") || detail.contains("parse"),
                    "unexpected detail: {detail}"
                );
            }
            other => panic!("expected RenderFailed, got {other:?}"),
        }
    }
}
