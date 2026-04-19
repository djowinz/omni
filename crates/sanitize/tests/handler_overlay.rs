//! Overlay handler tests against the real .omni multi-root format.

use sanitize::{sanitize_bundle, SanitizeError};

mod common;

// ----- Happy-path / structure acceptance --------------------------------

#[test]
fn accepts_minimal_widget_fragment() {
    let xml = br#"<widget><template><div/></template></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    sanitize_bundle(&m, f).expect("minimal widget-only fragment must sanitize");
}

#[test]
fn accepts_theme_config_widget_multi_root() {
    let xml = br#"<theme src="marathon.css"/><config><poll sensor="cpu.usage" interval="500"/></config><widget id="w1" name="W1" enabled="true"><template><div class="x"/></template><style>.x{color:red}</style></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    sanitize_bundle(&m, f).expect("theme+config+widget multi-root must sanitize");
}

#[test]
fn accepts_multiple_widgets() {
    let xml = br#"<widget id="a" name="A" enabled="true"><template><div/></template></widget><widget id="b" name="B" enabled="true"><template><span/></template></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    sanitize_bundle(&m, f).expect("multiple widget siblings must sanitize");
}

// ----- Top-level element rejection --------------------------------------

#[test]
fn rejects_unknown_top_level_element() {
    let xml = br#"<notanode/>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    match sanitize_bundle(&m, f).unwrap_err() {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(detail.contains("unexpected top-level"), "detail was: {detail}");
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

#[test]
fn rejects_unknown_config_child() {
    let xml = br#"<config><badchild sensor="x"/></config>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    match sanitize_bundle(&m, f).unwrap_err() {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(detail.contains("<config> child"), "detail was: {detail}");
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

#[test]
fn rejects_unknown_widget_child() {
    let xml = br#"<widget id="w" name="W" enabled="true"><notachild/></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    match sanitize_bundle(&m, f).unwrap_err() {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(detail.contains("<widget> child"), "detail was: {detail}");
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

#[test]
fn rejects_nested_widget() {
    let xml = br#"<widget id="a" name="A" enabled="true"><widget id="b" name="B" enabled="true"/></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    match sanitize_bundle(&m, f).unwrap_err() {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(detail.contains("<widget> child"), "detail was: {detail}");
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

// ----- <theme src=...> validation ---------------------------------------

#[test]
fn accepts_theme_with_relative_src() {
    let xml = br#"<theme src="marathon.css"/>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    sanitize_bundle(&m, f).expect("relative theme src must sanitize");
}

#[test]
fn rejects_theme_src_with_scheme() {
    let xml = br#"<theme src="http://evil"/>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    match sanitize_bundle(&m, f).unwrap_err() {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(detail.contains("relative workspace path"), "detail was: {detail}");
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

#[test]
fn rejects_theme_src_absolute() {
    let xml = br#"<theme src="/abs/path"/>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    match sanitize_bundle(&m, f).unwrap_err() {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(detail.contains("must be relative"), "detail was: {detail}");
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

#[test]
fn rejects_theme_src_with_parent_traversal() {
    let xml = br#"<theme src="../escape.css"/>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    match sanitize_bundle(&m, f).unwrap_err() {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(detail.contains(".."), "detail was: {detail}");
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

// ----- DOCTYPE / PI / CDATA rejection -----------------------------------

#[test]
fn rejects_doctype() {
    let xml = br#"<!DOCTYPE html SYSTEM "http://e"><widget/>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    assert!(matches!(
        sanitize_bundle(&m, f).unwrap_err(),
        SanitizeError::Handler { kind: "overlay", .. }
    ));
}

#[test]
fn rejects_cdata_in_envelope() {
    let xml = br#"<widget id="w" name="W" enabled="true"><![CDATA[x]]></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    assert!(matches!(
        sanitize_bundle(&m, f).unwrap_err(),
        SanitizeError::Handler { kind: "overlay", .. }
    ));
}

// ----- Envelope depth cap -----------------------------------------------

#[test]
fn rejects_envelope_depth_over_three() {
    let xml = br#"<config><poll sensor="a" interval="1"><extra/></poll></config>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    let err = sanitize_bundle(&m, f).unwrap_err();
    match err {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(
                detail.contains("envelope depth") || detail.contains("<poll>") || detail.contains("must be empty"),
                "expected depth-or-poll violation; detail was: {detail}"
            );
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

#[test]
fn accepts_deep_html_inside_template() {
    let mut inner = String::new();
    for _ in 0..30 {
        inner.push_str("<div>");
    }
    for _ in 0..30 {
        inner.push_str("</div>");
    }
    let xml = format!("<widget id=\"w\" name=\"W\" enabled=\"true\"><template>{inner}</template></widget>").into_bytes();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    sanitize_bundle(&m, f).expect("deep HTML inside template must sanitize");
}

// ----- Template body: script strip + directive-attr preservation --------

#[test]
fn strips_script_tag_in_template() {
    let xml = br#"<widget id="w" name="W" enabled="true"><template><div/><script>alert(1)</script></template></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    let (out, _r) = sanitize_bundle(&m, f).unwrap();
    let body = std::str::from_utf8(out.get("overlay.omni").unwrap()).unwrap();
    assert!(!body.contains("<script"), "script not stripped: {body}");
    assert!(body.contains("<div"), "div should survive: {body}");
}

#[test]
fn preserves_class_directive_attr() {
    let xml = br#"<widget id="w" name="W" enabled="true"><template><div class:gpu-hot="gpu.temp &gt;= 70">x</div></template></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    let (out, _r) = sanitize_bundle(&m, f).unwrap();
    let body = std::str::from_utf8(out.get("overlay.omni").unwrap()).unwrap();
    assert!(
        body.contains("class:gpu-hot"),
        "class:* directive attr must survive: {body}"
    );
}

#[test]
fn preserves_data_sensor_attrs() {
    let xml = br#"<widget id="w" name="W" enabled="true"><template><div data-sensor="cpu.usage" data-sensor-format="percent">x</div></template></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    let (out, _r) = sanitize_bundle(&m, f).unwrap();
    let body = std::str::from_utf8(out.get("overlay.omni").unwrap()).unwrap();
    assert!(body.contains("data-sensor=\"cpu.usage\""), "body was: {body}");
    assert!(body.contains("data-sensor-format=\"percent\""), "body was: {body}");
}

#[test]
fn preserves_chart_card_custom_element() {
    let xml = br#"<widget id="w" name="W" enabled="true"><template><chart-card sensor="cpu.usage" type="line" title="CPU"/></template></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    let (out, _r) = sanitize_bundle(&m, f).unwrap();
    let body = std::str::from_utf8(out.get("overlay.omni").unwrap()).unwrap();
    assert!(body.contains("chart-card"), "chart-card must survive: {body}");
    assert!(body.contains("sensor=\"cpu.usage\""), "body was: {body}");
    assert!(body.contains("type=\"line\""), "body was: {body}");
}

#[test]
fn preserves_svg_subtree() {
    let xml = br#"<widget id="w" name="W" enabled="true"><template><svg viewBox="0 0 100 100"><polyline points="0,0 10,10" stroke="red"/></svg></template></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    let (out, _r) = sanitize_bundle(&m, f).unwrap();
    let body = std::str::from_utf8(out.get("overlay.omni").unwrap()).unwrap();
    assert!(body.contains("<svg"), "body was: {body}");
    assert!(body.contains("<polyline"), "body was: {body}");
    assert!(body.contains("viewBox"), "body was: {body}");
    assert!(body.contains("points="), "body was: {body}");
    assert!(body.contains("stroke="), "body was: {body}");
}

// ----- Widget <style> body: CSS pipeline --------------------------------

#[test]
fn rejects_style_import_in_widget() {
    let xml = br#"<widget id="w" name="W" enabled="true"><template><div/></template><style>@import url('evil.css');</style></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    match sanitize_bundle(&m, f).unwrap_err() {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(detail.contains("@import"), "detail was: {detail}");
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

#[test]
fn rejects_style_external_url_in_widget() {
    let xml = br#"<widget id="w" name="W" enabled="true"><template><div/></template><style>body{background:url('http://evil/a.png')}</style></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    match sanitize_bundle(&m, f).unwrap_err() {
        SanitizeError::Handler { kind: "overlay", detail, .. } => {
            assert!(
                detail.contains("disallowed scheme") || detail.contains("url"),
                "detail was: {detail}"
            );
        }
        other => panic!("expected Handler error, got {other:?}"),
    }
}

#[test]
fn minifies_style_body() {
    let xml = br#"<widget id="w" name="W" enabled="true"><template><div/></template><style>  .foo  {  color : red ;  }  </style></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml);
    let (out, _r) = sanitize_bundle(&m, f).unwrap();
    let body = std::str::from_utf8(out.get("overlay.omni").unwrap()).unwrap();
    assert!(body.contains(".foo"), "body was: {body}");
    assert!(!body.contains("   "), "minify should collapse runs of spaces: {body}");
}

// ----- Byte identity outside sanitized bodies ---------------------------

#[test]
fn preserves_envelope_bytes_identically() {
    let xml = br#"<widget id="marathon-hud" name="Marathon HUD" enabled="true"><template><div/></template></widget>"#.to_vec();
    let (m, f) = common::bundle_with_overlay_bytes(xml.clone());
    let (out, _r) = sanitize_bundle(&m, f).unwrap();
    let body = std::str::from_utf8(out.get("overlay.omni").unwrap()).unwrap();
    assert!(
        body.starts_with(r#"<widget id="marathon-hud" name="Marathon HUD" enabled="true">"#),
        "envelope prefix must survive: {body}"
    );
}
