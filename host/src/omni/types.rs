//! Data types for the parsed .omni file format.
//! All types are JSON-serializable for Electron WebSocket communication.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A parsed .omni file containing a theme reference and widget definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniFile {
    /// Optional path to a theme CSS file.
    pub theme_src: Option<String>,
    /// Per-sensor poll interval configuration (sensor name -> interval in ms).
    pub poll_config: HashMap<String, u64>,
    /// Ordered list of widget definitions.
    pub widgets: Vec<Widget>,
}

/// A single widget definition with its template and scoped styles.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionalClass {
    pub class_name: String,
    pub expression: String,
}

/// A node in the HTML template tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        children: Vec<HtmlNode>,
    },
    Text {
        content: String,
    },
}

/// A resolved CSS property value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedStyle {
    // Position
    pub position: Option<String>, // "fixed", "relative"
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
    // Size
    pub width: Option<String>,
    pub height: Option<String>,
    // Visual
    pub background: Option<String>,
    pub color: Option<String>,
    pub opacity: Option<f32>,
    pub border_radius: Option<String>,
    // Typography
    pub font_size: Option<String>,
    pub font_weight: Option<String>,
    pub font_family: Option<String>,
    // Flexbox
    pub display: Option<String>,
    pub flex_direction: Option<String>,
    pub justify_content: Option<String>,
    pub align_items: Option<String>,
    pub gap: Option<String>,
    // Padding/margin
    pub padding: Option<String>,
    pub margin: Option<String>,
    // Min/max dimensions
    pub min_width: Option<String>,
    pub max_width: Option<String>,
    pub min_height: Option<String>,
    pub max_height: Option<String>,
    // Extended visual
    pub background_color: Option<String>,
    pub box_shadow: Option<String>,
    // Extended flexbox
    pub align_self: Option<String>,
    pub flex_grow: Option<String>,
    pub flex_shrink: Option<String>,
    pub flex_wrap: Option<String>,
    // Transitions
    pub transition: Option<String>,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        Self {
            position: None,
            top: None,
            right: None,
            bottom: None,
            left: None,
            width: None,
            height: None,
            background: None,
            color: None,
            opacity: None,
            border_radius: None,
            font_size: None,
            font_weight: None,
            font_family: None,
            display: None,
            flex_direction: None,
            justify_content: None,
            align_items: None,
            gap: None,
            padding: None,
            margin: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            background_color: None,
            box_shadow: None,
            align_self: None,
            flex_grow: None,
            flex_shrink: None,
            flex_wrap: None,
            transition: None,
        }
    }
}

impl OmniFile {
    /// Create an empty OmniFile.
    pub fn empty() -> Self {
        Self {
            theme_src: None,
            poll_config: HashMap::new(),
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
            children: vec![],
        };

        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("panel"));
        assert!(json.contains("active"));
    }
}
