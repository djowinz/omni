// These tests invoke ThemeHandler via the public sanitize_theme entry because
// handlers are crate-private. Until Task 9 wires sanitize_theme, this test
// file FAILS on running — expected per plan wave ordering. Task 3 has wired
// the test file to compile.

use omni_sanitize::{sanitize_theme, SanitizeError};

#[test]
fn rejects_import() {
    let err = sanitize_theme(b"@import url('evil.css'); body{}").unwrap_err();
    assert!(matches!(err, SanitizeError::Handler { kind: "theme", .. }));
}

#[test]
fn rejects_http_url() {
    let err = sanitize_theme(b"body{ background:url('https://x.com/e.png') }").unwrap_err();
    assert!(matches!(err, SanitizeError::Handler { kind: "theme", .. }));
}

#[test]
fn rejects_javascript_url() {
    let err = sanitize_theme(b"body{ background:url('javascript:alert(1)') }").unwrap_err();
    assert!(matches!(err, SanitizeError::Handler { kind: "theme", .. }));
}

#[test]
fn allows_relative_url_in_images_dir() {
    let (out, _r) = sanitize_theme(b"body{ background:url('images/bg.png') }").unwrap();
    assert!(std::str::from_utf8(&out).unwrap().contains("images/bg.png"));
}

#[test]
fn minifies_input() {
    let (out, _r) = sanitize_theme(b"body   {    color:  red;   }").unwrap();
    assert!(!std::str::from_utf8(&out).unwrap().contains("  "));
}
