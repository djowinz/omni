use omni_sanitize::{sanitize_bundle, SanitizeError};

mod common;

#[test]
fn rejects_dtd() {
    let xml = br#"<!DOCTYPE overlay SYSTEM "http://e"><overlay/>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    assert!(matches!(
        sanitize_bundle(&m, f).unwrap_err(),
        SanitizeError::Handler { kind: "overlay", .. }
    ));
}

#[test]
fn rejects_cdata() {
    let xml = br#"<overlay><template><![CDATA[<template>x</template>]]></template></overlay>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    assert!(matches!(
        sanitize_bundle(&m, f).unwrap_err(),
        SanitizeError::Handler { kind: "overlay", .. }
    ));
}

#[test]
fn rejects_deep_nesting() {
    let mut s = String::new();
    for _ in 0..20 { s.push_str("<div>"); }
    for _ in 0..20 { s.push_str("</div>"); }
    let xml = format!("<overlay><template>{s}</template></overlay>").into_bytes();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    assert!(matches!(
        sanitize_bundle(&m, f).unwrap_err(),
        SanitizeError::Handler { kind: "overlay", .. }
    ));
}

#[test]
fn accepts_minimal_and_strips_script() {
    let xml = br#"<overlay><template><div class="x"/><script>alert(1)</script></template><style>body{}</style></overlay>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    let (out, _r) = sanitize_bundle(&m, f).unwrap();
    let body = out.get("overlay.omni").unwrap();
    let s = std::str::from_utf8(body).unwrap();
    assert!(!s.contains("<script"), "script not stripped: {s}");
}

#[test]
fn preserves_data_sensor_attrs() {
    let xml = br#"<overlay><template><div data-sensor="cpu.usage" data-sensor-format="percent">x</div></template></overlay>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    let (out, _r) = sanitize_bundle(&m, f).unwrap();
    let body = std::str::from_utf8(out.get("overlay.omni").unwrap()).unwrap();
    assert!(body.contains("data-sensor=\"cpu.usage\""));
    assert!(body.contains("data-sensor-format=\"percent\""));
}
