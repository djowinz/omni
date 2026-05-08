//! Pretty-print CSS / HTML / .omni content for human editing. Called by the
//! install and fork pipelines to expand minified bytes into a readable form
//! on disk. NOT a security boundary: failures fall back to raw bytes.
//! For the upload-side security gate, see `crate::sanitize_bundle`.

mod css;
mod error;
mod html;
mod omni;

pub use error::BeautifyError;

use css::beautify_css;
use omni::beautify_omni;

/// True if `filename` will be transformed by `beautify_for_disk`. Lets callers
/// short-circuit to a pass-through write (or OS-level copy) for files that
/// won't be touched, avoiding the parse + heap copy.
pub fn is_beautifiable(filename: &str) -> bool {
    matches!(extension(filename).as_str(), "css" | "omni")
}

/// Returns formatted bytes for a recognized extension. Unknown extensions
/// pass through unchanged (returns `Ok(input.to_vec())`).
pub fn beautify_for_disk(filename: &str, bytes: &[u8]) -> Result<Vec<u8>, BeautifyError> {
    match extension(filename).as_str() {
        "css" => beautify_css(bytes),
        "omni" => beautify_omni(bytes),
        _ => Ok(bytes.to_vec()),
    }
}

fn extension(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn unknown_extension_passes_through() {
        let input = b"{\"name\":\"x\"}";
        let out = beautify_for_disk("manifest.json", input).expect("must succeed");
        assert_eq!(out, input);
    }

    #[test]
    fn no_extension_passes_through() {
        let input = b"some content";
        let out = beautify_for_disk("README", input).expect("must succeed");
        assert_eq!(out, input);
    }

    #[test]
    fn extension_match_is_case_insensitive() {
        let input = b"body{color:red}";
        let out_lower = beautify_for_disk("a.css", input).expect("lowercase ok");
        let out_upper = beautify_for_disk("a.CSS", input).expect("uppercase ok");
        assert_eq!(out_lower, out_upper);
    }

    #[test]
    fn is_beautifiable_matches_dispatch() {
        assert!(is_beautifiable("themes/dark.css"));
        assert!(is_beautifiable("widget.omni"));
        assert!(is_beautifiable("WIDGET.OMNI"));
        assert!(!is_beautifiable("manifest.json"));
        assert!(!is_beautifiable("README"));
        assert!(!is_beautifiable("preview.png"));
    }
}
