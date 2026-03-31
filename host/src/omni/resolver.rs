//! Resolves an OmniFile into ComputedWidgets for rendering.
//!
//! Pipeline: OmniFile → for each enabled widget → flatten template tree →
//! resolve CSS with full selector matching → interpolate sensor values →
//! emit ComputedWidget for each HTML element.

use std::collections::HashMap;

use omni_shared::{ComputedWidget, SensorSnapshot, WidgetType, SensorSource, write_fixed_str};
use tracing::warn;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
};
use windows::core::w;

use super::types::{OmniFile, ResolvedStyle};
use super::css;
use super::flat_tree::{self, FlatNode};
use super::interpolation;
use super::sensor_map;

/// Resolves an OmniFile into a flat list of ComputedWidgets.
pub struct OmniResolver {
    /// Theme CSS variables (loaded from theme file).
    theme_vars: HashMap<String, String>,
    /// DirectWrite factory for text measurement.
    dwrite_factory: Option<IDWriteFactory>,
}

impl OmniResolver {
    pub fn new() -> Self {
        let dwrite_factory: Option<IDWriteFactory> = unsafe {
            DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok()
        };
        if dwrite_factory.is_none() {
            warn!("Failed to create IDWriteFactory — text measurement will use fallback estimates");
        }
        Self {
            theme_vars: HashMap::new(),
            dwrite_factory,
        }
    }

    /// Measure text dimensions using DirectWrite.
    ///
    /// Returns `(width, height)` for the given text rendered at the specified
    /// font size and weight.  Falls back to a simple character-count estimate
    /// if DirectWrite is unavailable or any API call fails.
    pub fn measure_text(&self, text: &str, font_size: f32, font_weight: u16) -> (f32, f32) {
        if let Some(ref factory) = self.dwrite_factory {
            if let Ok(size) = Self::measure_text_dwrite(factory, text, font_size, font_weight) {
                return size;
            }
        }
        // Fallback: rough estimate
        (text.len() as f32 * font_size * 0.6, font_size + 4.0)
    }

    fn measure_text_dwrite(
        factory: &IDWriteFactory,
        text: &str,
        font_size: f32,
        font_weight: u16,
    ) -> Result<(f32, f32), windows::core::Error> {
        unsafe {
            let weight = if font_weight >= 700 {
                DWRITE_FONT_WEIGHT_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            };

            let format = factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size,
                w!("en-us"),
            )?;

            let text_wide: Vec<u16> = text.encode_utf16().collect();

            let layout = factory.CreateTextLayout(
                &text_wide,
                &format,
                10000.0,
                10000.0,
            )?;

            let mut metrics = std::mem::zeroed::<windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_METRICS>();
            layout.GetMetrics(&mut metrics)?;
            Ok((metrics.width, metrics.height))
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
            let flat_nodes = flat_tree::flatten_tree(&widget_def.template);

            // Track resolved positions and widths per node for child layout
            let mut positions: Vec<(f32, f32)> = vec![(0.0, 0.0); flat_nodes.len()];
            let mut parent_widths: Vec<f32> = vec![0.0; flat_nodes.len()];
            // Track resolved styles per node for height estimation
            let mut resolved_styles: Vec<Option<ResolvedStyle>> = vec![None; flat_nodes.len()];

            for (i, node) in flat_nodes.iter().enumerate() {
                if node.is_text {
                    continue; // text nodes handled by parent
                }

                // Resolve CSS with full selector matching
                let interpolated_inline = node.inline_style.as_ref()
                    .map(|s| interpolation::interpolate(s, snapshot));

                // Create a temporary node with interpolated inline style for CSS resolution
                let mut resolve_node = node.clone();
                resolve_node.inline_style = interpolated_inline;

                let style = css::resolve_styles(
                    &resolve_node, i, &flat_nodes, &stylesheet, &self.theme_vars,
                );

                // Compute position:
                // 1. If the element has explicit position (left/top), use it (fixed positioning)
                // 2. Otherwise, use the position assigned by the parent's flex layout
                let (assigned_x, assigned_y) = positions[i]; // position parent computed for us

                let x = parse_px(style.left.as_deref()).unwrap_or(assigned_x);
                let y = parse_px(style.top.as_deref()).unwrap_or(assigned_y);
                // Width: explicit > inherit from parent > fallback 0 (auto)
                let inherited_width = parent_widths[i];
                let width = parse_px(style.width.as_deref())
                    .unwrap_or(if inherited_width > 0.0 { inherited_width } else { 0.0 });
                let height = parse_px(style.height.as_deref()).unwrap_or(0.0);

                // For child positioning: account for padding and gap
                let padding = parse_px(style.padding.as_deref()).unwrap_or(0.0);
                let gap = parse_px(style.gap.as_deref()).unwrap_or(0.0);
                let is_row = style.flex_direction.as_deref() == Some("row");

                // Child content width = parent width minus padding on both sides
                let child_content_width = if width > 0.0 { width - padding * 2.0 } else { 0.0 };

                // Set initial child position (inside padding)
                let mut child_x = x + padding;
                let mut child_y = y + padding;

                // Update positions and widths for each direct child
                for &child_idx in &node.child_indices {
                    positions[child_idx] = (child_x, child_y);
                    parent_widths[child_idx] = child_content_width;

                    if is_row {
                        child_x += width + gap;
                    } else {
                        let ch = estimate_flat_node_height(&flat_nodes, child_idx, &style);
                        child_y += ch + gap;
                    }
                }

                // Check if this element has text children
                let has_text_children = node.child_indices.iter()
                    .any(|&idx| flat_nodes[idx].is_text);

                if has_text_children {
                    // Collect raw template text (before interpolation)
                    let raw_template: String = node.child_indices.iter()
                        .filter_map(|&idx| {
                            if flat_nodes[idx].is_text {
                                flat_nodes[idx].text_content.as_deref()
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    // Interpolate sensor values
                    let text = interpolation::interpolate(&raw_template, snapshot);

                    // Determine sensor source from text content
                    let source = detect_sensor_source_flat(&flat_nodes, node);

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
                    widgets.push(cw);
                } else if !node.child_indices.is_empty() {
                    // Container element — emit background if styled
                    let bg = parse_color(style.background.as_deref());
                    if bg[3] > 0 || style.border_radius.is_some() {
                        let mut cw = style_to_computed_widget(&style, x, y, width);
                        cw.widget_type = WidgetType::Group;
                        cw.source = SensorSource::None;

                        // Calculate container height from children
                        let child_height = estimate_children_height_flat(
                            &flat_nodes, node, &style,
                        );
                        cw.height = if height > 0.0 { height } else { child_height };

                        widgets.push(cw);
                    }
                } else {
                    // Empty element (spacer, decoration)
                    if height > 0.0 || parse_color(style.background.as_deref())[3] > 0 {
                        let mut cw = style_to_computed_widget(&style, x, y, width);
                        cw.widget_type = WidgetType::Spacer;
                        cw.height = height;
                        widgets.push(cw);
                    }
                }

                resolved_styles[i] = Some(style);
            }
        }

        widgets
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

    // Parse background: gradient takes priority, otherwise solid color
    if let Some(bg) = &style.background {
        let bg_trimmed = bg.trim();
        if bg_trimmed.starts_with("linear-gradient(") {
            if let Some(gradient) = parse_linear_gradient(bg_trimmed) {
                cw.bg_gradient = gradient;
            }
        } else {
            cw.bg_color_rgba = parse_color(Some(bg_trimmed));
        }
    }
    // background-color as fallback when no solid bg was set
    if cw.bg_color_rgba == [0, 0, 0, 0] && !cw.bg_gradient.enabled {
        if let Some(bg_color) = &style.background_color {
            cw.bg_color_rgba = parse_color(Some(bg_color));
        }
    }

    // Box shadow
    if let Some(shadow_str) = &style.box_shadow {
        if let Some(shadow) = parse_box_shadow(shadow_str) {
            cw.box_shadow = shadow;
        }
    }

    // Per-corner border radius
    if let Some(br) = &style.border_radius {
        cw.border_radius = parse_border_radius(br);
    }

    cw
}

/// Parse a CSS `linear-gradient(...)` value into a `GradientDef`.
fn parse_linear_gradient(value: &str) -> Option<omni_shared::GradientDef> {
    let inner = value.strip_prefix("linear-gradient(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.splitn(3, ',').collect();
    if parts.len() < 2 { return None; }

    let angle_str = parts[0].trim();
    let has_angle = angle_str.ends_with("deg") || angle_str.starts_with("to ");

    let angle_deg = if angle_str.ends_with("deg") {
        angle_str.trim_end_matches("deg").parse::<f32>().unwrap_or(180.0)
    } else if angle_str.starts_with("to ") {
        match angle_str {
            "to right" => 90.0,
            "to left" => 270.0,
            "to bottom" => 180.0,
            "to top" => 0.0,
            "to bottom right" => 135.0,
            "to top right" => 45.0,
            _ => 180.0,
        }
    } else {
        180.0
    };

    // Pick color strings based on whether an angle/direction was provided
    let color1_str = if has_angle {
        parts.get(1).map(|s| s.trim()).unwrap_or("")
    } else {
        parts.get(0).map(|s| s.trim()).unwrap_or("")
    };
    let color2_str = parts.last().map(|s| s.trim()).unwrap_or("");

    // Strip percentage suffixes from color stops
    let color1 = color1_str.split_whitespace().next().unwrap_or("");
    let color2 = color2_str.split_whitespace().next().unwrap_or("");

    Some(omni_shared::GradientDef {
        enabled: true,
        angle_deg,
        start_rgba: parse_color(Some(color1)),
        end_rgba: parse_color(Some(color2)),
    })
}

/// Parse a CSS `box-shadow` value into a `ShadowDef`.
fn parse_box_shadow(value: &str) -> Option<omni_shared::ShadowDef> {
    let parts: Vec<&str> = value.trim().splitn(4, ' ').collect();
    if parts.len() < 3 { return None; }

    let offset_x = parse_px(Some(parts[0])).unwrap_or(0.0);
    let offset_y = parse_px(Some(parts[1])).unwrap_or(0.0);
    let blur_radius = parse_px(Some(parts[2])).unwrap_or(0.0);

    let color_str = if parts.len() >= 4 { parts[3] } else { "rgba(0,0,0,0.5)" };
    let color_rgba = parse_color(Some(color_str));

    Some(omni_shared::ShadowDef {
        enabled: true,
        offset_x,
        offset_y,
        blur_radius,
        color_rgba,
    })
}

/// Parse a CSS `border-radius` shorthand with 1-4 values.
/// Returns `[top-left, top-right, bottom-right, bottom-left]`.
fn parse_border_radius(value: &str) -> [f32; 4] {
    let parts: Vec<f32> = value.split_whitespace()
        .filter_map(|s| parse_px(Some(s)))
        .collect();

    match parts.len() {
        1 => [parts[0]; 4],
        2 => [parts[0], parts[1], parts[0], parts[1]],
        3 => [parts[0], parts[1], parts[2], parts[1]],
        4 => [parts[0], parts[1], parts[2], parts[3]],
        _ => [0.0; 4],
    }
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

/// Detect the primary sensor source from text children of a flat node.
fn detect_sensor_source_flat(flat_nodes: &[FlatNode], node: &FlatNode) -> SensorSource {
    for &idx in &node.child_indices {
        if flat_nodes[idx].is_text {
            if let Some(ref content) = flat_nodes[idx].text_content {
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
    }
    SensorSource::None
}

/// Estimate the height of all children of a flat node for container sizing.
fn estimate_children_height_flat(
    flat_nodes: &[FlatNode],
    parent: &FlatNode,
    parent_style: &ResolvedStyle,
) -> f32 {
    let gap = parse_px(parent_style.gap.as_deref()).unwrap_or(0.0);
    let padding = parse_px(parent_style.padding.as_deref()).unwrap_or(0.0);

    let mut total = padding * 2.0;
    let count = parent.child_indices.len();
    for (idx_pos, &child_idx) in parent.child_indices.iter().enumerate() {
        total += estimate_flat_node_height(flat_nodes, child_idx, parent_style);
        if idx_pos < count - 1 {
            total += gap;
        }
    }
    total
}

/// Estimate the height of a single flat node.
fn estimate_flat_node_height(
    flat_nodes: &[FlatNode],
    node_idx: usize,
    parent_style: &ResolvedStyle,
) -> f32 {
    let node = &flat_nodes[node_idx];

    if node.is_text {
        return parse_px(parent_style.font_size.as_deref()).unwrap_or(14.0) + 8.0;
    }

    // Check for explicit height in inline style
    if let Some(ref style) = node.inline_style {
        if let Some(h) = style.split(';')
            .find(|s| s.trim().starts_with("height"))
            .and_then(|s| s.split(':').nth(1))
            .and_then(|v| parse_px(Some(v)))
        {
            return h;
        }
    }

    if node.child_indices.is_empty() {
        0.0
    } else {
        // Check if this node has direct text children (it's a text-bearing element)
        let has_text = node.child_indices.iter().any(|&idx| flat_nodes[idx].is_text);
        if has_text {
            let font_size = parse_px(parent_style.font_size.as_deref()).unwrap_or(14.0);
            font_size + 8.0
        } else {
            // Container with element children — sum their heights recursively
            let gap = node.inline_style.as_ref()
                .and_then(|s| s.split(';')
                    .find(|p| p.trim().starts_with("gap"))
                    .and_then(|p| p.split(':').nth(1))
                    .and_then(|v| parse_px(Some(v))))
                .unwrap_or(0.0);
            let padding = node.inline_style.as_ref()
                .and_then(|s| s.split(';')
                    .find(|p| p.trim().starts_with("padding"))
                    .and_then(|p| p.split(':').nth(1))
                    .and_then(|v| parse_px(Some(v))))
                .unwrap_or(0.0);

            let count = node.child_indices.len();
            let mut total = padding * 2.0;
            for (i, &child_idx) in node.child_indices.iter().enumerate() {
                total += estimate_flat_node_height(flat_nodes, child_idx, parent_style);
                if i < count - 1 {
                    total += gap;
                }
            }
            total
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

    #[test]
    fn descendant_selector_applies() {
        // .panel .label { color: red; } applies to span inside div.panel
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template>
                    <div class="panel">
                        <span class="label">hello</span>
                    </div>
                </template>
                <style>
                    .panel .label { color: red; }
                </style>
            </widget>
        "#;

        let file = parser::parse_omni(source).unwrap();
        let resolver = OmniResolver::new();
        let widgets = resolver.resolve(&file, &SensorSnapshot::default());

        let text_widget = widgets.iter()
            .find(|w| w.widget_type == WidgetType::SensorValue)
            .expect("Should have a SensorValue widget");
        assert_eq!(text_widget.color_rgba, [255, 0, 0, 255], "Descendant selector should apply red color");
    }

    #[test]
    fn compound_selector_applies() {
        // .value.critical { color: red; } applies to element with both classes
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template>
                    <div>
                        <span class="value critical">hot</span>
                        <span class="value">normal</span>
                    </div>
                </template>
                <style>
                    .value { color: white; }
                    .value.critical { color: red; }
                </style>
            </widget>
        "#;

        let file = parser::parse_omni(source).unwrap();
        let resolver = OmniResolver::new();
        let widgets = resolver.resolve(&file, &SensorSnapshot::default());

        let sensor_widgets: Vec<_> = widgets.iter()
            .filter(|w| w.widget_type == WidgetType::SensorValue)
            .collect();
        assert_eq!(sensor_widgets.len(), 2, "Should have 2 SensorValue widgets");

        // First widget (value critical) should be red
        assert_eq!(sensor_widgets[0].color_rgba, [255, 0, 0, 255],
            "Compound selector .value.critical should apply red");
        // Second widget (value only) should be white
        assert_eq!(sensor_widgets[1].color_rgba, [255, 255, 255, 255],
            "Simple .value should apply white");
    }

    #[test]
    fn specificity_wins() {
        // #id rule beats .class rule
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template>
                    <span class="val" id="main">text</span>
                </template>
                <style>
                    .val { font-size: 14px; }
                    #main { font-size: 24px; }
                </style>
            </widget>
        "#;

        let file = parser::parse_omni(source).unwrap();
        let resolver = OmniResolver::new();
        let widgets = resolver.resolve(&file, &SensorSnapshot::default());

        let w = &widgets[0];
        assert_eq!(w.font_size, 24.0, "ID selector should win over class selector");
    }

    // --- Gradient parsing tests ---

    #[test]
    fn parse_gradient_degrees_and_colors() {
        let g = parse_linear_gradient("linear-gradient(135deg, #ff0000, #0000ff)").unwrap();
        assert!(g.enabled);
        assert_eq!(g.angle_deg, 135.0);
        assert_eq!(g.start_rgba, [255, 0, 0, 255]);
        assert_eq!(g.end_rgba, [0, 0, 255, 255]);
    }

    #[test]
    fn parse_gradient_to_right_keyword() {
        let g = parse_linear_gradient("linear-gradient(to right, red, blue)").unwrap();
        assert_eq!(g.angle_deg, 90.0);
        assert_eq!(g.start_rgba, [255, 0, 0, 255]);
        assert_eq!(g.end_rgba, [0, 0, 255, 255]);
    }

    #[test]
    fn parse_gradient_with_percent_stops() {
        let g = parse_linear_gradient("linear-gradient(180deg, #ff0000 0%, #0000ff 100%)").unwrap();
        assert_eq!(g.angle_deg, 180.0);
        assert_eq!(g.start_rgba, [255, 0, 0, 255]);
        assert_eq!(g.end_rgba, [0, 0, 255, 255]);
    }

    #[test]
    fn parse_gradient_returns_none_for_invalid() {
        assert!(parse_linear_gradient("not-a-gradient").is_none());
        assert!(parse_linear_gradient("linear-gradient(red)").is_none());
    }

    // --- Box shadow parsing tests ---

    #[test]
    fn parse_box_shadow_with_rgba() {
        let s = parse_box_shadow("2px 4px 8px rgba(0,0,0,0.5)").unwrap();
        assert!(s.enabled);
        assert_eq!(s.offset_x, 2.0);
        assert_eq!(s.offset_y, 4.0);
        assert_eq!(s.blur_radius, 8.0);
        assert_eq!(s.color_rgba, [0, 0, 0, 127]);
    }

    #[test]
    fn parse_box_shadow_zeroes() {
        let s = parse_box_shadow("0 0 0").unwrap();
        assert!(s.enabled);
        assert_eq!(s.offset_x, 0.0);
        assert_eq!(s.offset_y, 0.0);
        assert_eq!(s.blur_radius, 0.0);
    }

    #[test]
    fn parse_box_shadow_too_few_parts() {
        assert!(parse_box_shadow("2px 4px").is_none());
    }

    // --- Border radius parsing tests ---

    #[test]
    fn parse_border_radius_one_value() {
        assert_eq!(parse_border_radius("8px"), [8.0, 8.0, 8.0, 8.0]);
    }

    #[test]
    fn parse_border_radius_two_values() {
        assert_eq!(parse_border_radius("8px 0"), [8.0, 0.0, 8.0, 0.0]);
    }

    #[test]
    fn parse_border_radius_three_values() {
        assert_eq!(parse_border_radius("8px 4px 2px"), [8.0, 4.0, 2.0, 4.0]);
    }

    #[test]
    fn parse_border_radius_four_values() {
        assert_eq!(parse_border_radius("8px 4px 2px 0"), [8.0, 4.0, 2.0, 0.0]);
    }

    // --- Background shorthand integration tests ---

    #[test]
    fn style_to_widget_solid_background() {
        let style = ResolvedStyle {
            background: Some("#ff0000".to_string()),
            ..Default::default()
        };
        let cw = style_to_computed_widget(&style, 0.0, 0.0, 100.0);
        assert_eq!(cw.bg_color_rgba, [255, 0, 0, 255]);
        assert!(!cw.bg_gradient.enabled);
    }

    #[test]
    fn style_to_widget_gradient_background() {
        let style = ResolvedStyle {
            background: Some("linear-gradient(90deg, #ff0000, #0000ff)".to_string()),
            ..Default::default()
        };
        let cw = style_to_computed_widget(&style, 0.0, 0.0, 100.0);
        assert!(cw.bg_gradient.enabled);
        assert_eq!(cw.bg_gradient.angle_deg, 90.0);
        assert_eq!(cw.bg_gradient.start_rgba, [255, 0, 0, 255]);
        assert_eq!(cw.bg_gradient.end_rgba, [0, 0, 255, 255]);
    }

    #[test]
    fn style_to_widget_background_color_fallback() {
        let style = ResolvedStyle {
            background_color: Some("#00ff00".to_string()),
            ..Default::default()
        };
        let cw = style_to_computed_widget(&style, 0.0, 0.0, 100.0);
        assert_eq!(cw.bg_color_rgba, [0, 255, 0, 255]);
    }

    #[test]
    fn style_to_widget_background_overrides_background_color() {
        let style = ResolvedStyle {
            background: Some("#ff0000".to_string()),
            background_color: Some("#00ff00".to_string()),
            ..Default::default()
        };
        let cw = style_to_computed_widget(&style, 0.0, 0.0, 100.0);
        // background should win over background_color
        assert_eq!(cw.bg_color_rgba, [255, 0, 0, 255]);
    }
}
