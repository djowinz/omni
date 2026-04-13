//! Per-View begin-loading filter.
//!
//! For sandboxed Views (`ViewTrust::is_sandboxed() == true`), we install
//! a `ulViewSetBeginLoadingCallback` that logs any attempt to load a
//! non-`file://` URL. The scoped FS already prevents those loads from
//! succeeding (any scheme other than `file` is rejected in
//! `OverlayFilesystem::resolve`), so this callback's job is observability
//! — give us a log line when a sandboxed bundle tries to reach out.

use ultralight_sys as ul;

use super::ul_string;
use super::view_trust::ViewTrust;

/// Install or remove the begin-loading callback for `view` based on `trust`.
/// Must be called after the View is created and before the first load.
///
/// # Safety
/// `view` must be a valid ULView pointer for the full lifetime over which
/// the callback may fire (i.e. until the View is destroyed or the callback
/// is replaced).
pub unsafe fn apply(view: ul::ULView, trust: ViewTrust) {
    if trust.is_sandboxed() {
        ul::ulViewSetBeginLoadingCallback(view, Some(cb_begin_loading), std::ptr::null_mut());
    } else {
        ul::ulViewSetBeginLoadingCallback(view, None, std::ptr::null_mut());
    }
}

unsafe extern "C" fn cb_begin_loading(
    _user_data: *mut std::os::raw::c_void,
    _caller: ul::ULView,
    _frame_id: std::os::raw::c_ulonglong,
    is_main_frame: bool,
    url: ul::ULString,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let url_str = ul_string::from_ul(url);
        if url_str.is_empty() {
            tracing::debug!("trust_filter: received null/empty URL in begin_loading");
            return;
        }
        if !is_allowed_url(&url_str) {
            tracing::warn!(url = %url_str, is_main_frame, "trust_filter: sandboxed View attempted non-file:// load");
        }
    }));
}

fn is_allowed_url(url: &str) -> bool {
    if url.is_empty() {
        return true;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("file://") {
        return true;
    }
    // about:blank has no content; a subsequent navigation away re-fires begin_loading with the new URL which this callback will re-evaluate.
    if lower == "about:blank"
        || lower.starts_with("about:blank#")
        || lower.starts_with("about:blank?")
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_scheme_allowed_any_case() {
        assert!(is_allowed_url("file:///C:/path/x.png"));
        assert!(is_allowed_url("file:///fonts/x.ttf"));
        assert!(is_allowed_url("File:///fonts/x.ttf"));
        assert!(is_allowed_url("FILE:///fonts/x.ttf"));
    }

    #[test]
    fn network_schemes_blocked() {
        assert!(!is_allowed_url("http://evil.com/x"));
        assert!(!is_allowed_url("https://evil.com/x"));
        assert!(!is_allowed_url("ws://evil.com/x"));
        assert!(!is_allowed_url("wss://evil.com/x"));
        assert!(!is_allowed_url("data:text/html,<script>"));
    }

    #[test]
    fn script_and_pseudo_schemes_blocked() {
        assert!(!is_allowed_url("javascript:alert(1)"));
        assert!(!is_allowed_url("JAVASCRIPT:alert(1)"));
        assert!(!is_allowed_url("vbscript:msgbox"));
        assert!(!is_allowed_url("blob:https://evil/uuid"));
        assert!(!is_allowed_url("filesystem:https://evil/persistent/x"));
    }

    #[test]
    fn about_blank_exact_and_fragments_allowed() {
        assert!(is_allowed_url("about:blank"));
        assert!(is_allowed_url("About:Blank"));
        assert!(is_allowed_url("ABOUT:BLANK"));
        assert!(is_allowed_url("about:blank#foo"));
        assert!(is_allowed_url("about:blank?x=1"));
    }

    #[test]
    fn about_blank_prefix_attack_rejected() {
        assert!(!is_allowed_url("about:blankfoo"));
        assert!(!is_allowed_url("about:blank.evil.com"));
    }

    #[test]
    fn empty_url_treated_as_allowed() {
        assert!(is_allowed_url(""));
    }
}
