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
use super::layout;
use super::sensor_map;
use super::transition;
use super::reactive;

/// Resolves an OmniFile into a flat list of ComputedWidgets.
pub struct OmniResolver {
    /// Theme CSS variables (loaded from theme file).
    theme_vars: HashMap<String, String>,
    /// DirectWrite factory for text measurement.
    dwrite_factory: Option<IDWriteFactory>,
    /// Transition engine for smooth property interpolation.
    transition_manager: transition::TransitionManager,
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
            transition_manager: transition::TransitionManager::new(),
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
    pub fn resolve(&mut self, file: &OmniFile, snapshot: &SensorSnapshot) -> Vec<ComputedWidget> {
        let mut widgets = Vec::new();

        for widget_def in &file.widgets {
            if !widget_def.enabled {
                continue;
            }

            let stylesheet = css::parse_css(&widget_def.style_source);
            let flat_nodes = flat_tree::flatten_tree(&widget_def.template);

            // Step 1-2: Resolve CSS for each non-text node
            let mut resolved_styles: Vec<Option<ResolvedStyle>> = vec![None; flat_nodes.len()];

            // Step 1b: Evaluate reactive classes for ALL elements first,
            // so descendant selectors (e.g., .hot .temp-value) see the
            // updated classes on ancestors during CSS resolution.
            let mut reactive_flat_nodes = flat_nodes.clone();
            for (i, node) in reactive_flat_nodes.iter_mut().enumerate() {
                if node.is_text { continue; }
                node.classes = reactive::resolve_active_classes(&flat_nodes[i], snapshot);
            }

            for (i, node) in reactive_flat_nodes.iter().enumerate() {
                if node.is_text {
                    continue;
                }

                let interpolated_inline = flat_nodes[i].inline_style.as_ref()
                    .map(|s| interpolation::interpolate(s, snapshot));

                let mut resolve_node = node.clone();
                resolve_node.inline_style = interpolated_inline;

                let mut style = css::resolve_styles(
                    &resolve_node, i, &reactive_flat_nodes, &stylesheet, &self.theme_vars,
                );

                // Apply transitions: if the style declares a transition property,
                // parse rules, compute current property map, and interpolate.
                if let Some(transition_str) = &style.transition.clone() {
                    let rules = transition::TransitionManager::parse_transition(transition_str);
                    let current_props = style_to_property_map(&style);
                    let overrides = self.transition_manager.update(
                        &widget_def.id, i, &rules, &current_props,
                    );
                    apply_property_overrides(&mut style, &overrides);
                }

                resolved_styles[i] = Some(style);
            }

            // Step 3: Measure text for text-bearing elements
            let mut text_sizes: Vec<(f32, f32)> = vec![(0.0, 0.0); flat_nodes.len()];
            for (i, node) in flat_nodes.iter().enumerate() {
                if node.is_text { continue; }
                let has_text = node.child_indices.iter().any(|&idx| flat_nodes[idx].is_text);
                if has_text {
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
                    let text = interpolation::interpolate(&raw_template, snapshot);
                    let style = resolved_styles[i].as_ref();
                    let font_size = style
                        .and_then(|s| parse_px(s.font_size.as_deref()))
                        .unwrap_or(14.0);
                    let font_weight: u16 = style
                        .and_then(|s| s.font_weight.as_deref())
                        .and_then(|w| match w {
                            "bold" => Some(700),
                            "normal" => Some(400),
                            _ => w.parse().ok(),
                        })
                        .unwrap_or(400);
                    let (tw, th) = self.measure_text(&text, font_size, font_weight);
                    // Add buffer: ceil + 4px for sub-pixel rounding between
                    // host measurement and DLL rendering at different DPIs.
                    // Also accounts for DLL frame timing override changing text length.
                    text_sizes[i] = (tw.ceil() + 4.0, th.ceil());
                }
            }

            // Step 4: Compute layout with taffy
            let styles_for_layout: Vec<ResolvedStyle> = resolved_styles.iter()
                .map(|s| s.clone().unwrap_or_default())
                .collect();

            let layouts = layout::compute_layout(
                &flat_nodes, &styles_for_layout, &text_sizes,
                1920.0, 1080.0,
            );

            // Step 5: Emit ComputedWidgets using layout positions
            for (i, node) in flat_nodes.iter().enumerate() {
                if node.is_text { continue; }

                let lo = &layouts[i];
                let style = resolved_styles[i].as_ref();
                let default_style = ResolvedStyle::default();
                let style = style.unwrap_or(&default_style);

                let x = lo.x;
                let y = lo.y;
                let width = lo.width;
                let height = lo.height;

                let has_text_children = node.child_indices.iter()
                    .any(|&idx| flat_nodes[idx].is_text);

                if has_text_children {
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

                    let text = interpolation::interpolate(&raw_template, snapshot);
                    let source = detect_sensor_source_flat(&flat_nodes, node);

                    // For text widgets, use the parent row's width as the draw rect
                    // instead of the element's taffy-computed width. Taffy sizes
                    // flex children to their content (min_size), but the D2D DrawText
                    // rect needs to be wide enough for the text not to clip.
                    // Left-aligned text renders correctly with extra width.
                    let text_draw_width = node.parent_index
                        .map(|pi| {
                            let parent_lo = &layouts[pi];
                            // Width from the element's left edge to the parent's right edge
                            (parent_lo.x + parent_lo.width) - x
                        })
                        .unwrap_or(width)
                        .max(width);

                    let mut cw = style_to_computed_widget(style, x, y, text_draw_width);
                    cw.widget_type = WidgetType::SensorValue;
                    cw.source = source;
                    cw.height = height;

                    write_fixed_str(&mut cw.label_text, &raw_template);
                    write_fixed_str(&mut cw.format_pattern, &text);
                    widgets.push(cw);
                } else if !node.child_indices.is_empty() {
                    let bg = parse_color(style.background.as_deref());
                    if bg[3] > 0 || style.border_radius.is_some() {
                        let mut cw = style_to_computed_widget(style, x, y, width);
                        cw.widget_type = WidgetType::Group;
                        cw.source = SensorSource::None;
                        cw.height = height;

                        widgets.push(cw);
                    }
                } else {
                    if height > 0.0 || parse_color(style.background.as_deref())[3] > 0 {
                        let mut cw = style_to_computed_widget(style, x, y, width);
                        cw.widget_type = WidgetType::Spacer;
                        cw.height = height;
                        widgets.push(cw);
                    }
                }
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

/// Split a string by a delimiter, but only at parenthesis depth 0.
/// This prevents splitting inside rgba(), linear-gradient(), etc.
fn split_at_depth_0(s: &str, delim: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, ch) in s.char_indices() {
        if ch == '(' { depth += 1; }
        else if ch == ')' { depth -= 1; }
        else if ch == delim && depth == 0 {
            parts.push(&s[start..i]);
            start = i + ch.len_utf8();
        }
    }
    parts.push(&s[start..]);
    parts
}

/// Parse a CSS `linear-gradient(...)` value into a `GradientDef`.
fn parse_linear_gradient(value: &str) -> Option<omni_shared::GradientDef> {
    let inner = value.strip_prefix("linear-gradient(")?.strip_suffix(')')?;
    let parts = split_at_depth_0(inner, ',');
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
/// Handles rgba() colors with spaces: `2px 4px 8px rgba(0, 0, 0, 0.5)`
fn parse_box_shadow(value: &str) -> Option<omni_shared::ShadowDef> {
    let v = value.trim();

    // Find the first 3 numeric values (offset_x, offset_y, blur_radius)
    // then everything remaining is the color
    let mut nums = Vec::new();
    let mut rest_start = 0;
    let mut chars = v.char_indices().peekable();

    while nums.len() < 3 {
        // Skip whitespace
        while let Some(&(_, ch)) = chars.peek() {
            if ch.is_whitespace() { chars.next(); } else { break; }
        }

        let start = match chars.peek() {
            Some(&(i, _)) => i,
            None => break,
        };

        // Collect until whitespace or paren (start of color)
        let mut end = start;
        while let Some(&(i, ch)) = chars.peek() {
            if ch.is_whitespace() || ch == '(' { break; }
            end = i + ch.len_utf8();
            chars.next();
        }

        if end > start {
            if let Some(val) = parse_px(Some(&v[start..end])) {
                nums.push(val);
                rest_start = end;
            } else {
                break;
            }
        } else {
            break;
        }
    }

    if nums.len() < 3 { return None; }

    let offset_x = nums[0];
    let offset_y = nums[1];
    let blur_radius = nums[2];

    let color_str = v[rest_start..].trim();
    let color_rgba = if color_str.is_empty() {
        parse_color(Some("rgba(0,0,0,0.5)"))
    } else {
        parse_color(Some(color_str))
    };

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

/// Extract animatable CSS property values from a ResolvedStyle into a property map.
fn style_to_property_map(style: &ResolvedStyle) -> HashMap<String, String> {
    let mut map = HashMap::new();
    macro_rules! insert_if_some {
        ($map:expr, $name:expr, $field:expr) => {
            if let Some(ref v) = $field {
                $map.insert($name.to_string(), v.clone());
            }
        };
    }
    insert_if_some!(map, "width", style.width);
    insert_if_some!(map, "height", style.height);
    insert_if_some!(map, "background", style.background);
    insert_if_some!(map, "background-color", style.background_color);
    insert_if_some!(map, "color", style.color);
    insert_if_some!(map, "border-radius", style.border_radius);
    insert_if_some!(map, "font-size", style.font_size);
    insert_if_some!(map, "padding", style.padding);
    insert_if_some!(map, "margin", style.margin);
    insert_if_some!(map, "gap", style.gap);
    insert_if_some!(map, "top", style.top);
    insert_if_some!(map, "right", style.right);
    insert_if_some!(map, "bottom", style.bottom);
    insert_if_some!(map, "left", style.left);
    insert_if_some!(map, "min-width", style.min_width);
    insert_if_some!(map, "max-width", style.max_width);
    insert_if_some!(map, "min-height", style.min_height);
    insert_if_some!(map, "max-height", style.max_height);
    insert_if_some!(map, "box-shadow", style.box_shadow);
    if let Some(opacity) = style.opacity {
        map.insert("opacity".to_string(), format!("{}", opacity));
    }
    map
}

/// Apply interpolated property overrides back onto a ResolvedStyle.
fn apply_property_overrides(style: &mut ResolvedStyle, overrides: &HashMap<String, String>) {
    for (key, value) in overrides {
        match key.as_str() {
            "width" => style.width = Some(value.clone()),
            "height" => style.height = Some(value.clone()),
            "background" => style.background = Some(value.clone()),
            "background-color" => style.background_color = Some(value.clone()),
            "color" => style.color = Some(value.clone()),
            "border-radius" => style.border_radius = Some(value.clone()),
            "font-size" => style.font_size = Some(value.clone()),
            "padding" => style.padding = Some(value.clone()),
            "margin" => style.margin = Some(value.clone()),
            "gap" => style.gap = Some(value.clone()),
            "top" => style.top = Some(value.clone()),
            "right" => style.right = Some(value.clone()),
            "bottom" => style.bottom = Some(value.clone()),
            "left" => style.left = Some(value.clone()),
            "min-width" => style.min_width = Some(value.clone()),
            "max-width" => style.max_width = Some(value.clone()),
            "min-height" => style.min_height = Some(value.clone()),
            "max-height" => style.max_height = Some(value.clone()),
            "box-shadow" => style.box_shadow = Some(value.clone()),
            "opacity" => {
                if let Ok(v) = value.parse::<f32>() {
                    style.opacity = Some(v);
                }
            }
            _ => {} // Unknown properties are ignored
        }
    }
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

        let mut resolver = OmniResolver::new();
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
        let mut resolver = OmniResolver::new();
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
        let mut resolver = OmniResolver::new();
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
        let mut resolver = OmniResolver::new();
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
        let mut resolver = OmniResolver::new();
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
