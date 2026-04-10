//! Converts a parsed OmniFile into HTML for Ultralight rendering.
//!
//! - `build_initial_html` — builds the full HTML page (called once at startup
//!   and on hot reload). Returns `InitialHtml` with separated html/css/full_document.
//! - `compute_update_diff` — walks the template tree with current sensor values
//!   and returns a structured `UpdateDiff` of element updates (called every cycle).
//! - `format_as_js` — serializes an `UpdateDiff` into a JS call for Ultralight.

use std::collections::HashMap;
use std::path::Path;

use super::expression;
use super::sensor_map;
use super::types::{HtmlNode, OmniFile};
use omni_shared::SensorSnapshot;

// ---------------------------------------------------------------------------
// Initial HTML (called once, or on hot reload)
// ---------------------------------------------------------------------------

/// Structured output from `build_initial_html`, giving callers access to the
/// widget markup and CSS independently of the full Ultralight document.
#[derive(Debug, Clone)]
pub struct InitialHtml {
    /// Widget markup with data-omni-id attributes, no wrapping html/body
    pub html: String,
    /// Combined widget styles + theme CSS
    pub css: String,
    /// Complete HTML document for Ultralight (html + css + omniUpdate JS)
    pub full_document: String,
}

/// Build the complete HTML page. This is loaded into Ultralight once.
/// The body contains all widget HTML with `data-omni-id` attributes for
/// targeted JS updates. A small `omniUpdate` function is embedded in a
/// `<script>` tag for receiving update payloads.
#[allow(clippy::too_many_arguments)]
pub fn build_initial_html(
    omni_file: &OmniFile,
    snapshot: &SensorSnapshot,
    viewport_width: u32,
    viewport_height: u32,
    data_dir: &Path,
    overlay_name: &str,
    hwinfo_values: &HashMap<String, f64>,
    hwinfo_units: &HashMap<String, String>,
) -> InitialHtml {
    let mut widget_css = String::new();
    let mut widget_html = String::new();
    let mut counter: u32 = 0;

    let theme_css = if let Some(ref theme_src) = omni_file.theme_src {
        load_theme_css(data_dir, overlay_name, theme_src)
    } else {
        String::new()
    };

    let feather_css = load_feather_css(data_dir);

    for widget in &omni_file.widgets {
        if !widget.enabled {
            continue;
        }
        widget_css.push_str(&widget.style_source);
        widget_css.push('\n');
        let html = render_initial_node(
            &widget.template,
            snapshot,
            &mut counter,
            hwinfo_values,
            hwinfo_units,
        );
        widget_html.push_str(&html);
        widget_html.push('\n');
    }

    // Combine all CSS for the structured output.
    // Include the same base reset as full_document so the preview renders identically.
    let css = format!(
        "*{{margin:0;padding:0;box-sizing:border-box}}\n{feather_css}\n{theme_css}\n{widget_css}",
        feather_css = feather_css,
        theme_css = theme_css,
        widget_css = widget_css,
    );

    let full_document = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
html,body{{width:{vw}px;height:{vh}px;background:transparent;overflow:hidden}}
{feather_css}
{theme_css}
{widget_css}
</style>
<script>
function omniUpdate(data) {{
    for (const [id, info] of Object.entries(data)) {{
        const el = document.querySelector('[data-omni-id="' + id + '"]');
        if (!el) continue;
        if (info.c !== undefined && el.className !== info.c) {{
            el.className = info.c;
        }}
        if (info.t !== undefined) {{
            for (const n of el.childNodes) {{
                if (n.nodeType === 3 && n.textContent !== info.t) {{
                    n.textContent = info.t;
                    break;
                }}
            }}
        }}
        if (info.a !== undefined) {{
            for (const [attr, val] of Object.entries(info.a)) {{
                el.setAttribute(attr, val);
            }}
        }}
    }}
}}
</script>
</head>
<body>
{widget_html}
</body>
</html>"#,
        vw = viewport_width,
        vh = viewport_height,
    );

    InitialHtml {
        html: widget_html,
        css,
        full_document,
    }
}

/// Render a node for the initial HTML. Evaluates reactive classes and
/// interpolates sensor values. Assigns `data-omni-id` to every element.
fn render_initial_node(
    node: &HtmlNode,
    snapshot: &SensorSnapshot,
    counter: &mut u32,
    hwinfo_values: &HashMap<String, f64>,
    hwinfo_units: &HashMap<String, String>,
) -> String {
    match node {
        HtmlNode::Text { content } => {
            if content.contains('{') {
                interpolate_with_hwinfo(content, snapshot, hwinfo_values, hwinfo_units)
            } else {
                content.clone()
            }
        }
        HtmlNode::Element {
            tag,
            id,
            classes,
            inline_style,
            conditional_classes,
            children,
        } => {
            let node_id = format!("omni-{}", *counter);
            *counter += 1;

            let mut active_classes = classes.clone();
            for cc in conditional_classes {
                if expression::eval_condition(&cc.expression, snapshot)
                    && !active_classes.contains(&cc.class_name)
                {
                    active_classes.push(cc.class_name.clone());
                }
            }

            let mut attrs = format!(r#" data-omni-id="{}""#, node_id);
            if let Some(ref el_id) = id {
                attrs.push_str(&format!(r#" id="{}""#, el_id));
            }
            if !active_classes.is_empty() {
                attrs.push_str(&format!(r#" class="{}""#, active_classes.join(" ")));
            }
            if let Some(ref style) = inline_style {
                attrs.push_str(&format!(r#" style="{}""#, style));
            }

            let children_html: String = children
                .iter()
                .map(|c| render_initial_node(c, snapshot, counter, hwinfo_values, hwinfo_units))
                .collect();

            if matches!(tag.as_str(), "br" | "hr" | "img" | "input") {
                format!("<{tag}{attrs} />")
            } else {
                format!("<{tag}{attrs}>{children_html}</{tag}>")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Per-cycle update types and functions
// ---------------------------------------------------------------------------

/// A single element's update: optional class list, text content, and/or SVG attributes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ElementUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub c: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub a: Option<HashMap<String, String>>,
}

/// A diff mapping omni-id → element update.
pub type UpdateDiff = HashMap<String, ElementUpdate>;

/// Compute the structured diff of element updates for the current sensor state.
/// Returns None if no elements need updating.
pub fn compute_update_diff(
    omni_file: &OmniFile,
    snapshot: &SensorSnapshot,
    hwinfo_values: &HashMap<String, f64>,
    hwinfo_units: &HashMap<String, String>,
) -> Option<UpdateDiff> {
    let mut diff = UpdateDiff::new();
    let mut counter: u32 = 0;

    for widget in &omni_file.widgets {
        if !widget.enabled {
            continue;
        }
        collect_diff_entries(&widget.template, snapshot, &mut counter, &mut diff, hwinfo_values, hwinfo_units);
    }

    if diff.is_empty() {
        None
    } else {
        Some(diff)
    }
}

/// Convert an `UpdateDiff` into the JS string format consumed by Ultralight:
/// `omniUpdate({"omni-0":{"c":"class1 class2","t":"72°C"},...})`
pub fn format_as_js(diff: &UpdateDiff) -> String {
    // Build entries in sorted order for deterministic output matching the
    // sequential omni-id assignment (omni-0, omni-1, ...).
    let mut ids: Vec<&String> = diff.keys().collect();
    ids.sort_by_key(|id| {
        id.strip_prefix("omni-")
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(u32::MAX)
    });

    let mut entries = String::new();
    for id in &ids {
        let update = &diff[*id];
        let mut parts = Vec::new();
        if let Some(ref c) = update.c {
            let escaped = c.replace('\\', "\\\\").replace('"', "\\\"");
            parts.push(format!(r#""c":"{}""#, escaped));
        }
        if let Some(ref t) = update.t {
            let escaped = t.replace('\\', "\\\\").replace('"', "\\\"");
            parts.push(format!(r#""t":"{}""#, escaped));
        }
        if let Some(ref a) = update.a {
            if !a.is_empty() {
                let mut attr_keys: Vec<&String> = a.keys().collect();
                attr_keys.sort();
                let attr_parts: Vec<String> = attr_keys
                    .iter()
                    .map(|k| {
                        let v = &a[*k];
                        let escaped_v = v.replace('\\', "\\\\").replace('"', "\\\"");
                        format!(r#""{}":"{}""#, k, escaped_v)
                    })
                    .collect();
                parts.push(format!(r#""a":{{{}}}"#, attr_parts.join(",")));
            }
        }
        if !parts.is_empty() {
            entries.push_str(&format!(r#""{}":{{{}}},"#, id, parts.join(",")));
        }
    }

    // Remove trailing comma
    if entries.ends_with(',') {
        entries.pop();
    }

    format!("omniUpdate({{{}}})", entries)
}

/// Walk the template tree and collect diff entries for elements that need updating.
fn collect_diff_entries(
    node: &HtmlNode,
    snapshot: &SensorSnapshot,
    counter: &mut u32,
    diff: &mut UpdateDiff,
    hwinfo_values: &HashMap<String, f64>,
    hwinfo_units: &HashMap<String, String>,
) {
    match node {
        HtmlNode::Text { .. } => {
            // Text nodes don't have IDs — they're updated via their parent element
        }
        HtmlNode::Element {
            classes,
            conditional_classes,
            children,
            ..
        } => {
            let node_id = format!("omni-{}", *counter);
            *counter += 1;

            let mut update_c: Option<String> = None;
            let mut update_t: Option<String> = None;

            // If this element has reactive classes, compute the full className
            if !conditional_classes.is_empty() {
                let mut active_classes = classes.clone();
                for cc in conditional_classes {
                    if expression::eval_condition(&cc.expression, snapshot)
                        && !active_classes.contains(&cc.class_name)
                    {
                        active_classes.push(cc.class_name.clone());
                    }
                }
                update_c = Some(active_classes.join(" "));
            }

            // If this element has a text child with sensor placeholders, interpolate it
            for child in children {
                if let HtmlNode::Text { content } = child {
                    if content.contains('{') {
                        let interpolated = interpolate_with_hwinfo(content, snapshot, hwinfo_values, hwinfo_units);
                        update_t = Some(interpolated);
                        break; // Only first text child
                    }
                }
            }

            if update_c.is_some() || update_t.is_some() {
                diff.insert(node_id, ElementUpdate { c: update_c, t: update_t, a: None });
            }

            // Recurse into children
            for child in children {
                collect_diff_entries(child, snapshot, counter, diff, hwinfo_values, hwinfo_units);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Replace all `{sensor.path}` expressions using hwinfo-aware lookup.
fn interpolate_with_hwinfo(
    input: &str,
    snapshot: &SensorSnapshot,
    hwinfo_values: &HashMap<String, f64>,
    hwinfo_units: &HashMap<String, String>,
) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut path = String::new();
            let mut found_close = false;
            for inner in chars.by_ref() {
                if inner == '}' {
                    found_close = true;
                    break;
                }
                path.push(inner);
            }

            if found_close && !path.is_empty() {
                let (sensor_path, precision) = parse_precision(path.trim());
                let value = sensor_map::get_sensor_value_with_hwinfo(
                    sensor_path,
                    snapshot,
                    hwinfo_values,
                    hwinfo_units,
                    precision,
                );
                result.push_str(&value);
            } else {
                result.push('{');
                result.push_str(&path);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

/// Parse precision suffix from a sensor path: `gpu.temp(2)` → `("gpu.temp", Some(2))`
fn parse_precision(input: &str) -> (&str, Option<usize>) {
    if let Some(paren_start) = input.rfind('(') {
        if input.ends_with(')') {
            let path = &input[..paren_start];
            if let Ok(n) = input[paren_start + 1..input.len() - 1].parse::<usize>() {
                return (path, Some(n));
            }
        }
    }
    (input, None)
}

fn load_theme_css(data_dir: &Path, overlay_name: &str, theme_src: &str) -> String {
    use crate::workspace::structure::resolve_theme_path;
    if let Some(path) = resolve_theme_path(data_dir, overlay_name, theme_src) {
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        String::new()
    }
}

fn load_feather_css(_data_dir: &Path) -> String {
    let mut css = String::new();

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));

    let font_path = exe_dir
        .as_ref()
        .map(|d| d.join("feather.ttf"))
        .filter(|p| p.exists())
        .or_else(|| {
            let dev_path = std::path::Path::new("crates/host/resources/feather.ttf");
            if dev_path.exists() {
                Some(dev_path.to_path_buf())
            } else {
                None
            }
        });

    if let Some(ref font) = font_path {
        if let Ok(font_bytes) = std::fs::read(font) {
            let b64 = simple_base64_encode(&font_bytes);
            css.push_str(&format!(
                r#"@font-face{{font-family:"feather";src:url("data:font/truetype;base64,{}") format("truetype");}}"#,
                b64
            ));
            css.push('\n');
        }
    }

    let css_path = exe_dir
        .as_ref()
        .map(|d| d.join("feather.css"))
        .filter(|p| p.exists())
        .or_else(|| {
            let dev_path = std::path::Path::new("crates/host/resources/feather.css");
            if dev_path.exists() {
                Some(dev_path.to_path_buf())
            } else {
                None
            }
        });

    if let Some(ref css_file) = css_path {
        if let Ok(full_css) = std::fs::read_to_string(css_file) {
            let class_defs = if let Some(face_start) = full_css.find("@font-face") {
                let mut brace_depth = 0;
                let mut end_pos = face_start;
                for (i, ch) in full_css[face_start..].char_indices() {
                    if ch == '{' {
                        brace_depth += 1;
                    }
                    if ch == '}' {
                        brace_depth -= 1;
                        if brace_depth == 0 {
                            end_pos = face_start + i + 1;
                            break;
                        }
                    }
                }
                &full_css[end_pos..]
            } else {
                &full_css
            };
            css.push_str(class_defs.trim_start_matches(['\n', '\r', ' ']));
        }
    }

    css
}

fn simple_base64_encode(data: &[u8]) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_update_with_attributes() {
        use std::collections::HashMap;
        let mut attrs = HashMap::new();
        attrs.insert("points".to_string(), "0,50 10,40 20,30".to_string());
        let update = ElementUpdate {
            c: None,
            t: None,
            a: Some(attrs),
        };
        let mut diff = UpdateDiff::new();
        diff.insert("omni-0".to_string(), update);
        let js = format_as_js(&diff);
        assert!(js.contains("\"a\":{"), "JS output missing 'a' field: {}", js);
        assert!(js.contains("\"points\":\"0,50 10,40 20,30\""), "JS output missing points value: {}", js);
    }

    #[test]
    fn element_update_without_attributes_omits_a_field() {
        let update = ElementUpdate {
            c: Some("active".to_string()),
            t: None,
            a: None,
        };
        let mut diff = UpdateDiff::new();
        diff.insert("omni-0".to_string(), update);
        let js = format_as_js(&diff);
        assert!(!js.contains("\"a\":"), "JS output should not contain 'a' field: {}", js);
    }

    #[test]
    fn element_update_with_class_text_and_attributes() {
        use std::collections::HashMap;
        let mut attrs = HashMap::new();
        attrs.insert("height".to_string(), "42".to_string());
        let update = ElementUpdate {
            c: Some("hot".to_string()),
            t: Some("72".to_string()),
            a: Some(attrs),
        };
        let mut diff = UpdateDiff::new();
        diff.insert("omni-0".to_string(), update);
        let js = format_as_js(&diff);
        assert!(js.contains("\"c\":\"hot\""));
        assert!(js.contains("\"t\":\"72\""));
        assert!(js.contains("\"a\":{"));
        assert!(js.contains("\"height\":\"42\""));
    }
}
