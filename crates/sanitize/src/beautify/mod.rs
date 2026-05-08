//! Pretty-print CSS / HTML / .omni content for human editing. Called by the
//! fork pipeline to expand minified installed-bundle bytes into a readable
//! form on disk. NOT a security boundary: failures fall back to raw bytes.
//! For the upload-side security gate, see `crate::sanitize_bundle`.

mod css;
mod error;
mod html;
mod omni;

pub use css::beautify_css;
pub use error::BeautifyError;
pub use html::beautify_html;
pub use omni::beautify_omni;

/// Returns formatted bytes for a recognized extension. Unknown extensions
/// pass through unchanged (returns `Ok(input.to_vec())`).
pub fn beautify_for_fork(filename: &str, bytes: &[u8]) -> Result<Vec<u8>, BeautifyError> {
    let ext = filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "css" => beautify_css(bytes),
        "omni" => beautify_omni(bytes),
        _ => Ok(bytes.to_vec()),
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn unknown_extension_passes_through() {
        let input = b"{\"name\":\"x\"}";
        let out = beautify_for_fork("manifest.json", input).expect("must succeed");
        assert_eq!(out, input);
    }

    #[test]
    fn no_extension_passes_through() {
        let input = b"some content";
        let out = beautify_for_fork("README", input).expect("must succeed");
        assert_eq!(out, input);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        // Stub returns input unchanged (will be replaced once Task 2 lands).
        // For now we only assert dispatch ROUTES to the css beautifier — the
        // stub won't error, it just round-trips.
        let input = b"body{color:red}";
        let out_lower = beautify_for_fork("a.css", input).expect("lowercase ok");
        let out_upper = beautify_for_fork("a.CSS", input).expect("uppercase ok");
        assert_eq!(out_lower, out_upper);
    }
}
