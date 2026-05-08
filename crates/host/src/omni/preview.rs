//! Preview broadcast payload for editor WebSocket subscribers.

use serde_json::{json, Value};
use std::collections::HashMap;

use super::html_builder::UpdateDiff;

/// Build the `preview.update.ingame` WebSocket message payload for a named
/// target channel (`"ingame"` or `"editor"`).
///
/// Shape is locked to `{ "type": "preview.update.<target>", "values": {...}, "diff": {...} }`.
/// `values` carries raw sensor values keyed by sensor path (consumed by the
/// editor's bootstrap-equivalent runtime — currently unused, see ts-002
/// follow-up). `diff` carries class/attr diffs per `data-omni-id` and is
/// consumed by `preview-updater.ts::applyPreviewDiff`.
pub fn build_preview_payload_for_target(
    target: &str,
    values: &HashMap<String, f64>,
    class_diff: Option<&UpdateDiff>,
) -> Value {
    let event_type = match target {
        "editor" => "preview.update.editor",
        _ => "preview.update.ingame",
    };
    json!({
        "type": event_type,
        "values": values,
        "diff": class_diff,
    })
}

/// Build the `preview.update.ingame` WebSocket message payload.
///
/// Delegates to [`build_preview_payload_for_target`] with target `"ingame"`.
/// Kept for call-site compatibility — existing callers are unaffected.
pub fn build_preview_payload(
    values: &HashMap<String, f64>,
    class_diff: Option<&UpdateDiff>,
) -> Value {
    build_preview_payload_for_target("ingame", values, class_diff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omni::html_builder::ElementUpdate;

    #[test]
    fn payload_shape_has_required_fields() {
        let mut values = HashMap::new();
        values.insert("cpu.usage".to_string(), 42.0);
        let payload = build_preview_payload(&values, None);
        assert_eq!(payload["type"], "preview.update.ingame");
        assert!(payload["values"].is_object());
        assert_eq!(payload["values"]["cpu.usage"], 42.0);
        assert!(payload["diff"].is_null());
    }

    #[test]
    fn payload_includes_diff_when_present() {
        let values = HashMap::new();
        let mut diff = UpdateDiff::new();
        diff.insert(
            "omni-0".into(),
            ElementUpdate {
                c: Some("sensor-warn".into()),
                t: None,
                a: None,
            },
        );
        let payload = build_preview_payload(&values, Some(&diff));
        assert_eq!(payload["diff"]["omni-0"]["c"], "sensor-warn");
    }

    #[test]
    fn target_helper_emits_correct_event_type() {
        let values = HashMap::new();
        let ingame = build_preview_payload_for_target("ingame", &values, None);
        assert_eq!(ingame["type"], "preview.update.ingame");
        let editor = build_preview_payload_for_target("editor", &values, None);
        assert_eq!(editor["type"], "preview.update.editor");
        // Unknown targets fall through to ingame.
        let fallback = build_preview_payload_for_target("unknown", &values, None);
        assert_eq!(fallback["type"], "preview.update.ingame");
    }
}
