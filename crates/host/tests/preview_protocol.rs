//! Integration: host → editor preview.update protocol contract.
//!
//! Pins the JSON shape emitted by the host on each tick and documents the
//! current gap in the editor consumer (`apps/desktop/renderer/lib/preview-updater.ts`):
//! sensor text updates flow through `values` but `preview-updater.ts` only
//! reads `diff.*.t` which the host no longer populates post-ts-002.
//! This test surfaces the regression for sub-spec #014 to close.

use std::collections::HashMap;

use omni_host::omni::html_builder::{ElementUpdate, UpdateDiff};
use omni_host::omni::preview::build_preview_payload;

#[test]
fn payload_pins_top_level_shape() {
    let mut values = HashMap::new();
    values.insert("cpu.usage".into(), 73.0);
    let payload = build_preview_payload(&values, None);
    let obj = payload.as_object().expect("object root");
    let keys: std::collections::BTreeSet<&str> =
        obj.keys().map(|k| k.as_str()).collect();
    let expected: std::collections::BTreeSet<&str> =
        ["type", "values", "diff"].into_iter().collect();
    assert_eq!(keys, expected, "preview.update payload shape drifted");
    assert_eq!(payload["type"], "preview.update");
}

#[test]
fn values_carry_sensor_paths_as_f64() {
    let mut values = HashMap::new();
    values.insert("cpu.usage".into(), 42.5);
    values.insert("gpu.temp".into(), 71.0);
    let payload = build_preview_payload(&values, None);
    assert_eq!(payload["values"]["cpu.usage"], 42.5);
    assert_eq!(payload["values"]["gpu.temp"], 71.0);
}

#[test]
fn diff_carries_class_updates_only_post_ts002() {
    // After ts-002, `update.t` is never populated by the host. The editor
    // consumer at preview-updater.ts reads `update.t` for text preview —
    // this means live text preview in the editor is effectively broken
    // until sub-spec #014 teaches the consumer to apply `values` too.
    let mut diff = UpdateDiff::new();
    diff.insert("omni-0".into(), ElementUpdate {
        c: Some("sensor-warn".into()),
        t: None,
        a: None,
    });
    let values = HashMap::new();
    let payload = build_preview_payload(&values, Some(&diff));
    assert_eq!(payload["diff"]["omni-0"]["c"], "sensor-warn");
    assert!(payload["diff"]["omni-0"].get("t").is_none()
         || payload["diff"]["omni-0"]["t"].is_null(),
        "update.t should not be populated post-ts-002 (tracked for #014)");
}

/// Structural compatibility sketch — not executable from Rust, but pins the
/// editor consumer's expected diff entry shape in a comment verified by the
/// asserts above.
///
/// ```typescript
/// interface PreviewDiff {
///   [omniId: string]: { c?: string; t?: string; a?: Record<string, string> };
/// }
/// ```
#[test]
fn diff_entry_schema_compatible_with_editor_consumer() {
    let mut diff = UpdateDiff::new();
    let mut attrs = HashMap::new();
    attrs.insert("value".to_string(), "73".to_string());
    diff.insert("omni-0".into(), ElementUpdate {
        c: Some("ok".into()),
        t: None,
        a: Some(attrs),
    });
    let values = HashMap::new();
    let payload = build_preview_payload(&values, Some(&diff));
    let entry = &payload["diff"]["omni-0"];
    // The editor reads `c` as a string, `a` as a record of strings. If any
    // of these becomes non-string, preview-updater.ts will silently no-op.
    assert!(entry["c"].is_string());
    assert!(entry["a"].is_object());
    assert!(entry["a"]["value"].is_string());
}
