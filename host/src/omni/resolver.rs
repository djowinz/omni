//! Resolves an OmniFile into ComputedWidgets for rendering.
//!
//! Pipeline: OmniFile → for each enabled widget → resolve CSS → interpolate
//! sensor values → emit ComputedWidget for each HTML element.

use std::collections::HashMap;

use omni_shared::{ComputedWidget, SensorSnapshot, WidgetType, SensorSource, write_fixed_str};

use super::types::{OmniFile, HtmlNode, ResolvedStyle};
use super::css::{self, ParsedStylesheet};
use super::interpolation;
use super::sensor_map;

/// Resolves an OmniFile into a flat list of ComputedWidgets.
pub struct OmniResolver {
    /// Theme CSS variables (loaded from theme file).
    theme_vars: HashMap<String, String>,
}

impl OmniResolver {
    pub fn new() -> Self {
        Self {
            theme_vars: HashMap::new(),
        }
    }

    /// Load theme variables from a CSS source string.
    pub fn load_theme(&mut self, theme_css: &str) {
        let sheet = css::parse_css(theme_css);
        self.theme_vars = sheet.variables;
    }

    /// Resolve the OmniFile into ComputedWidgets using current sensor data.
    pub fn resolve(&self, file: &OmniFile, snapshot: &SensorSnapshot) -> Vec<ComputedWidget> {
        let mut widgets = Vec::new();

        for widget_def in &file.widgets {
            if !widget_def.enabled {
                continue;
            }

            let stylesheet = css::parse_css(&widget_def.style_source);
            self.resolve_node(
                &widget_def.template,
                &stylesheet,
                snapshot,
                &mut widgets,
                0.0, 0.0, // parent offset
            );
        }

        widgets
    }

    fn resolve_node(
        &self,
        node: &HtmlNode,
        stylesheet: &ParsedStylesheet,
        snapshot: &SensorSnapshot,
        out: &mut Vec<ComputedWidget>,
        parent_x: f32,
        parent_y: f32,
    ) {
        match node {
            HtmlNode::Element { tag, id, classes, inline_style, children } => {
                // Resolve CSS for this element
                let interpolated_inline = inline_style.as_ref()
                    .map(|s| interpolation::interpolate(s, snapshot));

                let style = css::resolve_styles(
                    tag,
                    id.as_deref(),
                    classes,
                    interpolated_inline.as_deref(),
                    stylesheet,
                    &self.theme_vars,
                );

                // Compute position
                let x = parse_px(style.left.as_deref()).unwrap_or(parent_x);
                let y = parse_px(style.top.as_deref()).unwrap_or(parent_y);
                let width = parse_px(style.width.as_deref()).unwrap_or(200.0);
                let height = parse_px(style.height.as_deref()).unwrap_or(0.0);

                // Check if this is a container (has children) or a leaf
                let has_text_children = children.iter().any(|c| matches!(c, HtmlNode::Text { .. }));

                if has_text_children {
                    // Collect raw template text (before interpolation)
                    let raw_template: String = children.iter()
                        .filter_map(|c| match c {
                            HtmlNode::Text { content } => Some(content.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    // Interpolate sensor values
                    let text = interpolation::interpolate(&raw_template, snapshot);

                    // Determine sensor source from text content
                    let source = detect_sensor_source(&text, children);

                    let mut cw = style_to_computed_widget(&style, x, y, width);
                    cw.widget_type = WidgetType::SensorValue;
                    cw.source = source;

                    // Store the raw template in label_text so the DLL can
                    // interpolate frame timing placeholders while preserving
                    // the user's formatting (e.g., "{fps}: AVG" → "120: AVG")
                    write_fixed_str(&mut cw.label_text, &raw_template);

                    // Auto-calculate height if not specified
                    if height > 0.0 {
                        cw.height = height;
                    } else {
                        cw.height = cw.font_size + 8.0; // font size + padding
                    }

                    write_fixed_str(&mut cw.format_pattern, &text);
                    out.push(cw);
                } else if !children.is_empty() {
                    // Container element — emit background if styled, then process children
                    let bg = parse_color(style.background.as_deref());
                    if bg[3] > 0 || style.border_radius.is_some() {
                        // Emit a background widget for the container
                        let mut cw = style_to_computed_widget(&style, x, y, width);
                        cw.widget_type = WidgetType::Group;
                        cw.source = SensorSource::None;

                        // Calculate container height from children
                        let child_height = estimate_children_height(children, &style);
                        cw.height = if height > 0.0 { height } else { child_height };

                        out.push(cw);
                    }

                    // Layout children
                    let padding = parse_px(style.padding.as_deref()).unwrap_or(0.0);
                    let gap = parse_px(style.gap.as_deref()).unwrap_or(0.0);
                    let is_row = style.flex_direction.as_deref() == Some("row");

                    let mut child_x = x + padding;
                    let mut child_y = y + padding;

                    for child in children {
                        self.resolve_node(child, stylesheet, snapshot, out, child_x, child_y);

                        if is_row {
                            child_x += width + gap; // simplified: each child gets same width
                        } else {
                            // Estimate child height for vertical stacking
                            let ch = estimate_node_height(child, &style);
                            child_y += ch + gap;
                        }
                    }
                } else {
                    // Empty element (spacer, decoration)
                    if height > 0.0 || parse_color(style.background.as_deref())[3] > 0 {
                        let mut cw = style_to_computed_widget(&style, x, y, width);
                        cw.widget_type = WidgetType::Spacer;
                        cw.height = height;
                        out.push(cw);
                    }
                }
            }
            HtmlNode::Text { .. } => {
                // Text nodes are handled by their parent Element
            }
        }
    }
}

/// Convert a ResolvedStyle into a ComputedWidget with position and visual properties.
fn style_to_computed_widget(style: &ResolvedStyle, x: f32, y: f32, default_width: f32) -> ComputedWidget {
    let mut cw = ComputedWidget::default();
    cw.x = x;
    cw.y = y;
    cw.width = parse_px(style.width.as_deref()).unwrap_or(default_width);
    cw.opacity = style.opacity.unwrap_or(1.0);
    cw.font_size = parse_px(style.font_size.as_deref()).unwrap_or(14.0);
    cw.font_weight = style.font_weight.as_deref()
        .and_then(|w| match w {
            "bold" => Some(700),
            "normal" => Some(400),
            _ => w.parse().ok(),
        })
        .unwrap_or(400);
    cw.color_rgba = parse_color(style.color.as_deref());
    cw.bg_color_rgba = parse_color(style.background.as_deref());

    if let Some(br) = &style.border_radius {
        let r = parse_px(Some(br)).unwrap_or(0.0);
        cw.border_radius = [r; 4];
    }

    cw
}

/// Parse a CSS color value into [r, g, b, a].
fn parse_color(value: Option<&str>) -> [u8; 4] {
    let value = match value {
        Some(v) => v.trim(),
        None => return [0, 0, 0, 0],
    };

    // Hex colors
    if value.starts_with('#') {
        return parse_hex_color(value);
    }

    // rgba(r, g, b, a)
    if value.starts_with("rgba(") {
        if let Some(inner) = value.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() == 4 {
                let r = parts[0].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let g = parts[1].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let b = parts[2].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let a = (parts[3].trim().parse::<f32>().unwrap_or(1.0) * 255.0) as u8;
                return [r, g, b, a];
            }
        }
    }

    // rgb(r, g, b)
    if value.starts_with("rgb(") {
        if let Some(inner) = value.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() == 3 {
                let r = parts[0].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let g = parts[1].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let b = parts[2].trim().parse::<f32>().unwrap_or(0.0) as u8;
                return [r, g, b, 255];
            }
        }
    }

    // Named colors (common ones)
    match value {
        "white" => [255, 255, 255, 255],
        "black" => [0, 0, 0, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "blue" => [0, 0, 255, 255],
        "transparent" => [0, 0, 0, 0],
        _ => [0, 0, 0, 0],
    }
}

fn parse_hex_color(hex: &str) -> [u8; 4] {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).unwrap_or(0);
            [r, g, b, 255]
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            [r, g, b, 255]
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            let a = u8::from_str_radix(&hex[6..8], 16).unwrap_or(255);
            [r, g, b, a]
        }
        _ => [0, 0, 0, 0],
    }
}

/// Parse a CSS pixel value (e.g., "10px", "14") into f32.
fn parse_px(value: Option<&str>) -> Option<f32> {
    let v = value?.trim();
    let v = v.strip_suffix("px").unwrap_or(v);
    v.parse().ok()
}

/// Detect the primary sensor source from text content.
fn detect_sensor_source(_text: &str, children: &[HtmlNode]) -> SensorSource {
    // Look for {sensor.path} in the original template text
    for child in children {
        if let HtmlNode::Text { content } = child {
            if let Some(start) = content.find('{') {
                if let Some(end) = content[start..].find('}') {
                    let path = content[start + 1..start + end].trim();
                    if let Some(source) = sensor_map::parse_sensor_path(path) {
                        return source;
                    }
                }
            }
        }
    }
    SensorSource::None
}

/// Estimate the height of child nodes for container sizing.
fn estimate_children_height(children: &[HtmlNode], parent_style: &ResolvedStyle) -> f32 {
    let gap = parse_px(parent_style.gap.as_deref()).unwrap_or(0.0);
    let padding = parse_px(parent_style.padding.as_deref()).unwrap_or(0.0);

    let mut total = padding * 2.0;
    let count = children.len();
    for (i, child) in children.iter().enumerate() {
        total += estimate_node_height(child, parent_style);
        if i < count - 1 {
            total += gap;
        }
    }
    total
}

fn estimate_node_height(node: &HtmlNode, parent_style: &ResolvedStyle) -> f32 {
    match node {
        HtmlNode::Text { .. } => {
            parse_px(parent_style.font_size.as_deref()).unwrap_or(14.0) + 8.0
        }
        HtmlNode::Element { children, inline_style, .. } => {
            // Check for explicit height in inline style
            if let Some(style) = inline_style {
                if let Some(h) = style.split(';')
                    .find(|s| s.trim().starts_with("height"))
                    .and_then(|s| s.split(':').nth(1))
                    .and_then(|v| parse_px(Some(v)))
                {
                    return h;
                }
            }
            if children.is_empty() {
                0.0
            } else {
                let font_size = parse_px(parent_style.font_size.as_deref()).unwrap_or(14.0);
                font_size + 8.0 // default text element height
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::parser;

    #[test]
    fn resolve_simple_widget() {
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template>
                    <div class="panel" style="position: fixed; top: 10px; left: 20px;">
                        <span class="val">CPU: {cpu.usage}%</span>
                    </div>
                </template>
                <style>
                    .panel { background: rgba(20,20,20,180); border-radius: 8px; padding: 8px; }
                    .val { color: white; font-size: 14px; }
                </style>
            </widget>
        "#;

        let file = parser::parse_omni(source).unwrap();
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 42.0;

        let resolver = OmniResolver::new();
        let widgets = resolver.resolve(&file, &snapshot);

        assert!(!widgets.is_empty(), "Should produce at least one widget");

        // Check that the text was interpolated
        let text_widget = widgets.iter()
            .find(|w| w.widget_type == WidgetType::SensorValue)
            .expect("Should have a SensorValue widget");
        let text = omni_shared::read_fixed_str(&text_widget.format_pattern);
        assert!(text.contains("42"), "Should contain interpolated CPU value, got: {text}");
    }

    #[test]
    fn disabled_widget_produces_nothing() {
        let source = r#"
            <widget id="test" name="Test" enabled="false">
                <template><span>Hidden</span></template>
                <style></style>
            </widget>
        "#;

        let file = parser::parse_omni(source).unwrap();
        let resolver = OmniResolver::new();
        let widgets = resolver.resolve(&file, &SensorSnapshot::default());
        assert!(widgets.is_empty());
    }

    #[test]
    fn theme_variables_resolve() {
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template><span class="val">test</span></template>
                <style>.val { color: var(--text); }</style>
            </widget>
        "#;

        let file = parser::parse_omni(source).unwrap();
        let mut resolver = OmniResolver::new();
        resolver.load_theme(":root { --text: #ff0000; }");

        let widgets = resolver.resolve(&file, &SensorSnapshot::default());
        let w = &widgets[0];
        assert_eq!(w.color_rgba, [255, 0, 0, 255], "Should resolve theme variable to red");
    }

    #[test]
    fn parse_color_formats() {
        assert_eq!(parse_color(Some("#ff0000")), [255, 0, 0, 255]);
        assert_eq!(parse_color(Some("#f00")), [255, 0, 0, 255]);
        assert_eq!(parse_color(Some("rgba(255, 0, 0, 0.5)")), [255, 0, 0, 127]);
        assert_eq!(parse_color(Some("white")), [255, 255, 255, 255]);
        assert_eq!(parse_color(Some("transparent")), [0, 0, 0, 0]);
        assert_eq!(parse_color(None), [0, 0, 0, 0]);
    }
}
