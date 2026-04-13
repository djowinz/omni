//! Per-View begin-loading filter.
//!
//! For sandboxed Views (`ViewTrust::is_sandboxed() == true`), we install
//! a `ulViewSetBeginLoadingCallback` that logs any attempt to load a
//! non-`file://` URL. The scoped FS already prevents those loads from
//! succeeding (any scheme other than `file` is rejected in
//! `OverlayFilesystem::resolve`), so this callback's job is observability
//! — give us a log line when a sandboxed bundle tries to reach out.

use tracing::warn;
use ultralight_sys as ul;

use super::view_trust::ViewTrust;

/// Install or remove the begin-loading callback for `view` based on `trust`.
/// Must be called after the View is created and before the first load.
///
/// # Safety
/// `view` must be a valid ULView pointer for the full lifetime over which
/// the callback may fire (i.e. until the View is destroyed or the callback
/// is replaced).
#[allow(dead_code)]
pub unsafe fn apply(view: ul::ULView, trust: ViewTrust) {
    if trust.is_sandboxed() {
        ul::ulViewSetBeginLoadingCallback(
            view,
            Some(cb_begin_loading),
            std::ptr::null_mut(),
        );
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
    let url_str = ul_string_to_string(url);
    if !is_allowed_url(&url_str) {
        warn!(url = %url_str, is_main_frame, "trust_filter: sandboxed View attempted non-file:// load");
    }
}

fn is_allowed_url(url: &str) -> bool {
    url.starts_with("file://") || url.starts_with("about:blank") || url.is_empty()
}

unsafe fn ul_string_to_string(s: ul::ULString) -> String {
    if s.is_null() { return String::new(); }
    let data = ul::ulStringGetData(s);
    let len = ul::ulStringGetLength(s);
    if data.is_null() || len == 0 { return String::new(); }
    let slice = std::slice::from_raw_parts(data as *const u8, len);
    String::from_utf8_lossy(slice).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_scheme_allowed() {
        assert!(is_allowed_url("file:///C:/path/x.png"));
        assert!(is_allowed_url("file:///fonts/x.ttf"));
    }

    #[test]
    fn network_schemes_blocked() {
        assert!(!is_allowed_url("http://evil.com/x"));
        assert!(!is_allowed_url("https://evil.com/x"));
        assert!(!is_allowed_url("ws://evil.com/x"));
        assert!(!is_allowed_url("data:text/html,<script>"));
    }

    #[test]
    fn about_blank_allowed() {
        assert!(is_allowed_url("about:blank"));
        assert!(is_allowed_url(""));
    }
}
