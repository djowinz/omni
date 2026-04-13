//! Integration-style tests that chain the full sensor-to-JS rendering
//! pipeline without initializing Ultralight.
//!
//! Covers: widget tree → data-sensor lowering in `html_builder` →
//! bootstrap script injection → `collect_sensor_values` → `format_values_js`.

#![cfg(test)]

use std::collections::HashMap;
use std::path::Path;

use omni_shared::SensorSnapshot;

use super::history::SensorHistory;
use super::html_builder::{build_initial_html, collect_sensor_values, format_values_js, format_classes_js, compute_update_diff};
use super::types::{HtmlNode, OmniFile, Widget};
use super::view_trust::ViewTrust;

fn overlay_with_cpu_span() -> OmniFile {
    OmniFile {
        theme_src: None,
        poll_config: Default::default(),
        widgets: vec![Widget {
            id: "w".into(),
            name: "w".into(),
            enabled: true,
            template: HtmlNode::Element {
                tag: "div".into(),
                id: Some("root".into()),
                classes: vec![],
                inline_style: None,
                conditional_classes: vec![],
                attributes: vec![],
                children: vec![HtmlNode::Text { content: "CPU: {cpu.usage}%".into() }],
            },
            style_source: String::new(),
        }],
    }
}

#[test]
fn trusted_pipeline_emits_bootstrap_sensor_span_and_values() {
    let omni = overlay_with_cpu_span();
    let mut snap = SensorSnapshot::default();
    snap.cpu.total_usage_percent = 42.0;
    let hv: HashMap<String, f64> = HashMap::new();
    let hu: HashMap<String, String> = HashMap::new();
    let history = SensorHistory::new();

    let doc = build_initial_html(
        &omni, &snap, 400, 200,
        Path::new("."), "t", &hv, &hu,
        &history,
        ViewTrust::LocalAuthored,
    );

    // Bootstrap is injected and trusted.
    assert!(doc.full_document.contains("window.__omni_update"));
    assert!(doc.full_document.contains("const TRUSTED = true;"));

    // Lowering produced a data-sensor span in the widget HTML.
    assert!(doc.html.contains(r#"data-sensor="cpu.usage""#));
    assert!(doc.html.contains(r#"data-sensor-format="percent""#));

    // Value collection finds the lowered path.
    let values = collect_sensor_values(&omni, &snap, &hv);
    assert_eq!(values.get("cpu.usage"), Some(&42.0));

    // JSON formatting is a valid `__omni_update({...})` call.
    let js = format_values_js(&values);
    assert!(js.starts_with("__omni_update("));
    assert!(js.ends_with(")"));
    assert!(js.contains("\"cpu.usage\""));
}

#[test]
fn untrusted_pipeline_defangs_environment() {
    let omni = overlay_with_cpu_span();
    let snap = SensorSnapshot::default();
    let hv: HashMap<String, f64> = HashMap::new();
    let hu: HashMap<String, String> = HashMap::new();
    let history = SensorHistory::new();

    let doc = build_initial_html(
        &omni, &snap, 400, 200,
        Path::new("."), "t", &hv, &hu,
        &history,
        ViewTrust::BundleInstalled,
    );

    assert!(doc.full_document.contains("const TRUSTED = false;"));
    assert!(doc.full_document.contains("eval disabled"));
    // Legacy non-privileged update function is gone.
    assert!(!doc.full_document.contains("function omniUpdate"));
}

#[test]
fn class_diff_pipeline_produces_set_classes_call() {
    use super::types::ConditionalClass;
    let omni = OmniFile {
        theme_src: None,
        poll_config: Default::default(),
        widgets: vec![Widget {
            id: "w".into(),
            name: "w".into(),
            enabled: true,
            template: HtmlNode::Element {
                tag: "div".into(),
                id: None,
                classes: vec!["base".into()],
                inline_style: None,
                conditional_classes: vec![ConditionalClass {
                    class_name: "sensor-warn".into(),
                    expression: "cpu.usage >= 80".into(),
                }],
                attributes: vec![],
                children: vec![HtmlNode::Text { content: "{cpu.usage}%".into() }],
            },
            style_source: String::new(),
        }],
    };
    let mut snap = SensorSnapshot::default();
    snap.cpu.total_usage_percent = 90.0;
    let hv: HashMap<String, f64> = HashMap::new();
    let hu: HashMap<String, String> = HashMap::new();

    let diff = compute_update_diff(&omni, &snap, &hv, &hu).expect("diff");
    let js = format_classes_js(&diff).expect("classes js");
    assert!(js.starts_with("__omni_set_classes("));
    assert!(js.contains("sensor-warn"));
}
