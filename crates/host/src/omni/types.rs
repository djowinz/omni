//! Data types for the parsed .omni file format.
//! All types are JSON-serializable for Electron WebSocket communication.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Per-overlay device scale directive parsed from `<dpi-scale value="..."/>`
/// inside the `<config>` block. `Auto` resolves to the current monitor's DPI
/// at view creation/recreation time. `Manual(f)` is a literal float in the
/// range [0.5, 4.0].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum DpiScale {
    Auto,
    Manual(f64),
}

/// A parsed .omni file containing a theme reference and widget definitions.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct OmniFile {
    /// Optional path to a theme CSS file.
    pub theme_src: Option<String>,
    /// Per-sensor poll interval configuration (sensor name -> interval in ms).
    pub poll_config: HashMap<String, u64>,
    /// Per-overlay device scale opt-in. `None` (default, absent in source)
    /// preserves today's behavior (scale = 1.0). `Some(...)` triggers
    /// `ulViewConfigSetInitialDeviceScale` on the next view creation +
    /// CSS-pixel body sizing in `build_initial_html`.
    #[serde(default)]
    pub dpi_scale: Option<DpiScale>,
    /// Ordered list of widget definitions.
    pub widgets: Vec<Widget>,
}

/// A single widget definition with its template and scoped styles.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct Widget {
    /// Unique identifier (required).
    pub id: String,
    /// Human-readable display name (required).
    pub name: String,
    /// Whether this widget is rendered.
    pub enabled: bool,
    /// Root of the HTML template tree.
    pub template: HtmlNode,
    /// CSS rules scoped to this widget.
    pub style_source: String,
}

/// A conditional class binding parsed from `class:name="expression"` attributes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct ConditionalClass {
    pub class_name: String,
    pub expression: String,
}

/// A node in the HTML template tree.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
#[serde(tag = "type")]
pub enum HtmlNode {
    Element {
        tag: String,
        id: Option<String>,
        classes: Vec<String>,
        /// Inline style attribute value (unparsed CSS).
        inline_style: Option<String>,
        /// Conditional class bindings (`class:name="expr"`).
        conditional_classes: Vec<ConditionalClass>,
        /// Arbitrary attributes beyond tag/id/class/style (e.g., SVG points,
        /// d, width, height, viewBox). May contain `{...}` interpolations.
        /// Stored as a `Vec` of `(name, value)` pairs to preserve insertion
        /// order for deterministic diffs.
        attributes: Vec<(String, String)>,
        children: Vec<HtmlNode>,
    },
    Text {
        content: String,
    },
}

impl OmniFile {
    /// Create an empty OmniFile.
    pub fn empty() -> Self {
        Self {
            theme_src: None,
            poll_config: HashMap::new(),
            dpi_scale: None,
            widgets: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omni_file_serializes_to_json() {
        let file = OmniFile {
            theme_src: Some("./themes/dark.css".to_string()),
            poll_config: HashMap::new(),
            dpi_scale: None,
            widgets: vec![Widget {
                id: "fps".to_string(),
                name: "FPS Counter".to_string(),
                enabled: true,
                template: HtmlNode::Element {
                    tag: "div".to_string(),
                    id: Some("fps".to_string()),
                    classes: vec![],
                    inline_style: None,
                    conditional_classes: vec![],
                    attributes: vec![],
                    children: vec![HtmlNode::Text {
                        content: "{fps}".to_string(),
                    }],
                },
                style_source: "#fps { color: white; }".to_string(),
            }],
        };

        let json = serde_json::to_string(&file).unwrap();
        let deserialized: OmniFile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.widgets.len(), 1);
        assert_eq!(deserialized.widgets[0].id, "fps");
    }

    #[test]
    fn html_node_element_serializes() {
        let node = HtmlNode::Element {
            tag: "div".to_string(),
            id: None,
            classes: vec!["panel".to_string(), "active".to_string()],
            inline_style: Some("color: red;".to_string()),
            conditional_classes: vec![],
            attributes: vec![],
            children: vec![],
        };

        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("panel"));
        assert!(json.contains("active"));
    }
}
