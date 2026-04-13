//! Renders the privileged bootstrap script for injection into Ultralight Views.
//!
//! The script defines `__omni_update`, `__omni_set_classes`, `__omni_set_theme`,
//! and `__omni_rescan`, and (for untrusted Views) defangs the JS environment.

use super::view_trust::ViewTrust;

const BOOTSTRAP_SRC: &str = include_str!("bootstrap.js");
const TRUST_PLACEHOLDER: &str = "__OMNI_VIEW_TRUSTED__";

/// Render the bootstrap script body with the trust flag substituted.
/// The returned string is the raw JS — callers wrap it in `<script>` tags.
pub fn render(trust: ViewTrust) -> String {
    let flag = if trust.is_sandboxed() { "false" } else { "true" };
    BOOTSTRAP_SRC.replacen(TRUST_PLACEHOLDER, flag, 1)
}

/// Render the bootstrap wrapped in a `<script>` tag suitable for splicing into
/// the `<head>` of an Ultralight document.
pub fn render_script_tag(trust: ViewTrust) -> String {
    format!("<script>\n{}\n</script>", render(trust))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trusted_substitutes_true() {
        let js = render(ViewTrust::LocalAuthored);
        assert!(js.contains("const TRUSTED = true;"));
        assert!(!js.contains("__OMNI_VIEW_TRUSTED__"));
    }

    #[test]
    fn untrusted_substitutes_false() {
        let js = render(ViewTrust::BundleInstalled);
        assert!(js.contains("const TRUSTED = false;"));
        assert!(!js.contains("__OMNI_VIEW_TRUSTED__"));
    }

    #[test]
    fn thumbnail_gen_is_untrusted() {
        let js = render(ViewTrust::ThumbnailGen);
        assert!(js.contains("const TRUSTED = false;"));
    }

    #[test]
    fn exports_required_globals() {
        let js = render(ViewTrust::LocalAuthored);
        assert!(js.contains("window.__omni_update"));
        assert!(js.contains("window.__omni_set_classes"));
        assert!(js.contains("window.__omni_set_theme"));
        assert!(js.contains("window.__omni_rescan"));
    }

    #[test]
    fn script_tag_wraps_body() {
        let tag = render_script_tag(ViewTrust::LocalAuthored);
        assert!(tag.starts_with("<script>"));
        assert!(tag.ends_with("</script>"));
        assert!(tag.contains("window.__omni_update"));
    }

    #[test]
    fn placeholder_replaced_exactly_once() {
        // Guards against an attacker-controlled constant accidentally containing
        // the placeholder string.
        let js = render(ViewTrust::BundleInstalled);
        assert_eq!(js.matches("__OMNI_VIEW_TRUSTED__").count(), 0);
    }
}
