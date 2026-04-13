//! Safe helpers around Ultralight's `ULString` C-API handles.
//!
//! Used by the FS dispatcher and trust filter — both translate between
//! Rust `&str` and Ultralight's opaque string type.

use std::ffi::CString;
use ultralight_sys as ul;

/// Decode a `ULString` into an owned Rust `String` using lossy UTF-8.
/// Returns an empty string on null input or zero length — safe across FFI.
///
/// # Safety
/// `s` must be a valid pointer returned from Ultralight (or null).
pub unsafe fn from_ul(s: ul::ULString) -> String {
    if s.is_null() {
        return String::new();
    }
    let data = ul::ulStringGetData(s);
    let len = ul::ulStringGetLength(s);
    if data.is_null() || len == 0 {
        return String::new();
    }
    let slice = std::slice::from_raw_parts(data as *const u8, len);
    String::from_utf8_lossy(slice).into_owned()
}

/// Encode a `&str` into a freshly-allocated `ULString`.
/// Interior NUL bytes fall back to an empty string.
///
/// # Safety
/// Caller is responsible for destroying the returned `ULString`
/// via `ulDestroyString` (or handing it to Ultralight, which takes
/// ownership in some callback return positions).
pub unsafe fn to_ul(s: &str) -> ul::ULString {
    match CString::new(s) {
        Ok(c) => ul::ulCreateString(c.as_ptr()),
        Err(_) => ul::ulCreateString(c"".as_ptr()),
    }
}
