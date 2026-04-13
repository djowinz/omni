//! Converts a parsed OmniFile into HTML for Ultralight rendering.
//!
//! - `build_initial_html` — builds the full HTML page (called once at startup
//!   and on hot reload). Returns `InitialHtml` with separated html/css/full_document.
//! - `compute_update_diff` — walks the template tree with current sensor values
//!   and returns a structured `UpdateDiff` of element updates (called every cycle).

use std::collections::HashMap;
use std::path::Path;

use super::expression;
use super::history::SensorHistory;
use super::interpolation::{interpolate, EvalCtx};
use super::types::{HtmlNode, OmniFile};
use omni_shared::SensorSnapshot;

/// Default styling for `<chart>` and `<chart-card>` elements. Loaded before
/// user widget styles so users can override any rule. Provides a reasonable
/// out-of-the-box appearance on dark game overlays and the Electron preview.
const DEFAULT_CHART_CSS: &str = r#"
.omni-chart-line-stroke{stroke:#00D9FF;stroke-width:2;fill:none}
.omni-chart-bar-track{fill:rgba(255,255,255,0.06)}
.omni-chart-bar-fill{fill:#00D9FF;transition:height 300ms ease-out,y 300ms ease-out}
.omni-chart-pie-track{stroke:rgba(255,255,255,0.1)}
.omni-chart-pie-fill{stroke:#00D9FF;transition:stroke-dashoffset 500ms ease-out}
.omni-chart-card{font-family:system-ui,-apple-system,sans-serif}
.omni-chart-card-title{font-size:10px;font-weight:600;fill:#e5e5e5;letter-spacing:0.5px}
.omni-chart-card-y-label{font-size:8px;fill:#888}
.omni-chart-card-x-label{font-size:8px;fill:#666;letter-spacing:0.3px}
"#;

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
/// targeted JS updates. The privileged bootstrap script is injected before
/// `<style>` in `<head>` based on the supplied trust level.
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
    history: &SensorHistory,
    trust: crate::omni::view_trust::ViewTrust,
) -> InitialHtml {
    let bootstrap = crate::omni::js_bootstrap::render_script_tag(trust);
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
            history,
        );
        widget_html.push_str(&html);
        widget_html.push('\n');
    }

    // Combine all CSS for the structured output.
    // Include the same base reset as full_document so the preview renders identically.
    // Order: reset → feather → chart defaults → theme → widget. Widgets and themes
    // override chart defaults; chart defaults override feather icons.
    let css = format!(
        "*{{margin:0;padding:0;box-sizing:border-box}}\n{feather_css}\n{chart_css}\n{theme_css}\n{widget_css}",
        feather_css = feather_css,
        chart_css = DEFAULT_CHART_CSS,
        theme_css = theme_css,
        widget_css = widget_css,
    );

    let full_document = format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
{bootstrap}
<style>
*{{margin:0;padding:0;box-sizing:border-box}}
html,body{{width:{vw}px;height:{vh}px;background:transparent;overflow:hidden}}
{feather_css}
{chart_css}
{theme_css}
{widget_css}
</style>
</head>
<body>
{widget_html}
</body>
</html>"#,
        bootstrap = bootstrap,
        vw = viewport_width,
        vh = viewport_height,
        chart_css = DEFAULT_CHART_CSS,
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
    history: &SensorHistory,
) -> String {
    match node {
        HtmlNode::Text { content } => {
            if let Some(segments) = lower_text_to_segments(content) {
                let mut out = String::new();
                for seg in segments {
                    match seg {
                        TextSegment::Literal(s) => out.push_str(&s),
                        TextSegment::Sensor { path, format, precision } => {
                            let initial = super::sensor_map::get_sensor_value_with_hwinfo(
                                &path, snapshot, hwinfo_values, hwinfo_units, Some(precision),
                            );
                            let formatted = format_initial(&initial, format, precision);
                            out.push_str(&format!(
                                r#"<span data-sensor="{path}" data-sensor-format="{format}" data-sensor-precision="{precision}">{formatted}</span>"#,
                                path = html_escape(&path),
                                format = format,
                                precision = precision,
                                formatted = html_escape(&formatted),
                            ));
                        }
                    }
                }
                out
            } else if content.contains('{') {
                let ctx = EvalCtx { snapshot, history, hwinfo_values, hwinfo_units };
                interpolate(content, &ctx)
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
            attributes,
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

            // Emit arbitrary attributes (e.g., SVG points/d/viewBox/width).
            // Values may contain `{...}` interpolations — evaluate them so
            // the initial HTML matches the first update tick.
            for (name, value) in attributes {
                let resolved = if value.contains('{') {
                    let ctx = EvalCtx {
                        snapshot,
                        history,
                        hwinfo_values,
                        hwinfo_units,
                    };
                    interpolate(value, &ctx)
                } else {
                    value.clone()
                };
                attrs.push_str(&format!(
                    r#" {}="{}""#,
                    name,
                    resolved.replace('"', "&quot;")
                ));
            }

            let children_html: String = children
                .iter()
                .map(|c| render_initial_node(c, snapshot, counter, hwinfo_values, hwinfo_units, history))
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
///
/// Only populates the `c` (class) field for conditional classes. Text updates
/// now flow through `collect_sensor_values` / `format_values_js` via the
/// bootstrap's `__omni_update(values)` API.
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

/// Render a class-only `UpdateDiff` as a `__omni_set_classes({...})` call, or
/// return `None` if empty.
pub fn format_classes_js(diff: &UpdateDiff) -> Option<String> {
    let mut ids: Vec<&String> = diff.keys().collect();
    ids.sort_by_key(|id| {
        id.strip_prefix("omni-").and_then(|n| n.parse::<u32>().ok()).unwrap_or(u32::MAX)
    });
    let mut parts = Vec::new();
    for id in ids {
        if let Some(c) = &diff[id].c {
            let escaped = c.replace('\\', "\\\\").replace('"', "\\\"");
            parts.push(format!(r#""{}":"{}""#, id, escaped));
        }
    }
    if parts.is_empty() { None }
    else { Some(format!("__omni_set_classes({{{}}})", parts.join(","))) }
}

/// Render a sensor-values map as a `__omni_update({...})` call.
pub fn format_values_js(values: &HashMap<String, f64>) -> String {
    let json = serde_json::to_string(values).unwrap_or_else(|_| "{}".into());
    format!("__omni_update({})", json)
}

/// Walk the widget tree and collect every sensor path that any `data-sensor`
/// span in the rendered document is bound to, with its current raw numeric
/// value. Keys are sensor paths; values are `f64`.
pub fn collect_sensor_values(
    omni_file: &OmniFile,
    snapshot: &SensorSnapshot,
    hwinfo_values: &HashMap<String, f64>,
) -> HashMap<String, f64> {
    let mut out: HashMap<String, f64> = HashMap::new();
    for widget in &omni_file.widgets {
        if !widget.enabled { continue; }
        collect_paths(&widget.template, &mut out, snapshot, hwinfo_values);
    }
    out
}

fn collect_paths(
    node: &HtmlNode,
    out: &mut HashMap<String, f64>,
    snapshot: &SensorSnapshot,
    hwinfo_values: &HashMap<String, f64>,
) {
    match node {
        HtmlNode::Text { content } => {
            if let Some(segs) = lower_text_to_segments(content) {
                for seg in segs {
                    if let TextSegment::Sensor { path, .. } = seg {
                        if let Some(v) = raw_value(&path, snapshot, hwinfo_values) {
                            out.insert(path, v);
                        }
                    }
                }
            }
        }
        HtmlNode::Element { children, .. } => {
            for c in children { collect_paths(c, out, snapshot, hwinfo_values); }
        }
    }
}

fn raw_value(
    path: &str,
    snapshot: &SensorSnapshot,
    hwinfo_values: &HashMap<String, f64>,
) -> Option<f64> {
    if let Some(v) = hwinfo_values.get(path) { return Some(*v); }
    Some(match path {
        "cpu.usage" => snapshot.cpu.total_usage_percent as f64,
        "cpu.temp" => {
            let v = snapshot.cpu.package_temp_c as f64;
            if v.is_nan() { return None; } else { v }
        }
        "gpu.usage" => snapshot.gpu.usage_percent as f64,
        "gpu.temp" => {
            let v = snapshot.gpu.temp_c as f64;
            if v.is_nan() { return None; } else { v }
        }
        "gpu.clock" => snapshot.gpu.core_clock_mhz as f64,
        "gpu.mem-clock" => snapshot.gpu.mem_clock_mhz as f64,
        "gpu.vram.used" => snapshot.gpu.vram_used_mb as f64,
        "gpu.vram.total" => snapshot.gpu.vram_total_mb as f64,
        "gpu.power" => snapshot.gpu.power_draw_w as f64,
        "gpu.fan" => snapshot.gpu.fan_speed_percent as f64,
        "ram.usage" => snapshot.ram.usage_percent as f64,
        "ram.used" => snapshot.ram.used_mb as f64,
        "ram.total" => snapshot.ram.total_mb as f64,
        "fps" if snapshot.frame.available => snapshot.frame.fps as f64,
        "frame-time" if snapshot.frame.available => snapshot.frame.frame_time_ms as f64,
        "frame-time.avg" if snapshot.frame.available => snapshot.frame.frame_time_avg_ms as f64,
        "frame-time.1pct" if snapshot.frame.available => snapshot.frame.frame_time_1percent_ms as f64,
        "frame-time.01pct" if snapshot.frame.available => snapshot.frame.frame_time_01percent_ms as f64,
        _ => return None,
    })
}


/// Walk the template tree and collect diff entries for elements that need updating.
/// Only populates `c` (conditional classes) and `a` (attribute interpolations).
/// Text updates now flow via `collect_sensor_values` + `format_values_js`.
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
            // Text nodes don't have IDs — sensor text flows via __omni_update(values)
        }
        HtmlNode::Element {
            classes,
            conditional_classes,
            attributes,
            children,
            ..
        } => {
            let node_id = format!("omni-{}", *counter);
            *counter += 1;

            let mut update_c: Option<String> = None;

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

            // Walk arbitrary attributes — any value containing `{...}` needs
            // to be re-evaluated each tick and emitted as an `a` update.
            // Defer HashMap allocation until the first interpolatable
            // attribute is found, since most elements have none.
            let mut update_a: Option<HashMap<String, String>> = None;
            for (name, value) in attributes {
                if value.contains('{') {
                    let ctx = EvalCtx {
                        snapshot,
                        history: &SensorHistory::new(),
                        hwinfo_values,
                        hwinfo_units,
                    };
                    let interpolated = interpolate(value, &ctx);
                    update_a
                        .get_or_insert_with(HashMap::new)
                        .insert(name.clone(), interpolated);
                }
            }

            if update_c.is_some() || update_a.is_some() {
                diff.insert(
                    node_id,
                    ElementUpdate {
                        c: update_c,
                        t: None,
                        a: update_a,
                    },
                );
            }

            // Recurse into children
            for child in children {
                collect_diff_entries(child, snapshot, counter, diff, hwinfo_values, hwinfo_units);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sensor placeholder lowering helpers
// ---------------------------------------------------------------------------

/// Parse `path` or `path(N)` from a placeholder body. Returns the path
/// string and an optional precision override.
fn parse_precision(body: &str) -> (&str, Option<usize>) {
    if let Some(paren) = body.find('(') {
        if body.ends_with(')') {
            let path = body[..paren].trim();
            let prec_str = body[paren + 1..body.len() - 1].trim();
            if let Ok(n) = prec_str.parse::<usize>() {
                return (path, Some(n));
            }
        }
    }
    (body, None)
}

/// Infer (format, default_precision) for a built-in sensor path.
/// Returns `None` if the path is composite (e.g. `gpu.vram`) or unknown.
fn infer_sensor_format(path: &str) -> Option<(&'static str, usize)> {
    if path == "gpu.vram" { return None; }
    if path.starts_with("hwinfo.") { return Some(("raw", 0)); }
    if path.ends_with(".usage") || path.ends_with(".pct") || path.ends_with(".fan") || path == "ram.usage" {
        return Some(("percent", 0));
    }
    if path.ends_with(".temp") { return Some(("temperature", 0)); }
    if path.ends_with(".clock") || path.ends_with(".freq") || path.ends_with(".mem-clock") {
        return Some(("raw", 0));
    }
    if path.starts_with("frame-time") { return Some(("raw", 1)); }
    if path == "fps" { return Some(("raw", 0)); }
    if path == "cpu.usage" { return Some(("percent", 0)); }
    None
}

/// Parse `{sensor.path}` or `{sensor.path(N)}` body text into a list of
/// segments suitable for lowering. Returns `None` if the text contains no
/// recognizable sensor placeholders.
///
/// Segments are either literal text or an inferred sensor binding.
fn lower_text_to_segments(text: &str) -> Option<Vec<TextSegment>> {
    let mut segments: Vec<TextSegment> = Vec::new();
    let mut buf = String::new();
    let mut chars = text.chars().peekable();
    let mut any_sensor = false;

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut body = String::new();
            let mut closed = false;
            for inner in chars.by_ref() {
                if inner == '}' { closed = true; break; }
                body.push(inner);
            }
            if !closed {
                buf.push('{');
                buf.push_str(&body);
                continue;
            }
            let (path, prec_override) = parse_precision(body.trim());
            if let Some((fmt, default_prec)) = infer_sensor_format(path) {
                if !buf.is_empty() {
                    segments.push(TextSegment::Literal(std::mem::take(&mut buf)));
                }
                segments.push(TextSegment::Sensor {
                    path: path.to_string(),
                    format: fmt,
                    precision: prec_override.unwrap_or(default_prec),
                });
                any_sensor = true;
            } else {
                // Unsupported binding — preserve original text so existing
                // interpolation handles it.
                buf.push('{');
                buf.push_str(&body);
                buf.push('}');
            }
        } else {
            buf.push(ch);
        }
    }
    if !buf.is_empty() { segments.push(TextSegment::Literal(buf)); }
    if any_sensor { Some(segments) } else { None }
}

#[derive(Debug, Clone)]
enum TextSegment {
    Literal(String),
    Sensor { path: String, format: &'static str, precision: usize },
}

/// Escape a value for safe HTML attribute / text inclusion.
fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// `sensor_map` already returns a formatted string; keep the raw string for
/// the initial render. For numeric formats the JS bootstrap will overwrite on
/// first tick. This function just appends the unit suffix that matches JS.
fn format_initial(raw: &str, format: &str, _precision: usize) -> String {
    match format {
        "percent" if raw != "N/A" => format!("{}%", raw),
        "temperature" if raw != "N/A" => format!("{}\u{00B0}C", raw),
        _ => raw.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
    fn compute_update_diff_emits_attribute_changes() {
        // history removed from compute_update_diff in Task 7 — attributes still
        // interpolate but history-backed min/max default to 0.
        use crate::omni::types::{HtmlNode, OmniFile, Widget};
        use omni_shared::SensorSnapshot;
        use std::collections::HashMap;

        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 60.0;
        let hv = HashMap::new();
        let hu = HashMap::new();

        let file = OmniFile {
            theme_src: None,
            poll_config: HashMap::new(),
            widgets: vec![Widget {
                id: "bar".to_string(),
                name: "Bar".to_string(),
                enabled: true,
                template: HtmlNode::Element {
                    tag: "svg".to_string(),
                    id: None,
                    classes: vec![],
                    inline_style: None,
                    conditional_classes: vec![],
                    attributes: vec![],
                    children: vec![HtmlNode::Element {
                        tag: "rect".to_string(),
                        id: None,
                        classes: vec![],
                        inline_style: None,
                        conditional_classes: vec![],
                        attributes: vec![
                            (
                                "height".to_string(),
                                "{bar_height(cpu.usage, 60, 0, 100)}".to_string(),
                            ),
                            (
                                "y".to_string(),
                                "{bar_y(cpu.usage, 60, 0, 100)}".to_string(),
                            ),
                        ],
                        children: vec![],
                    }],
                },
                style_source: String::new(),
            }],
        };

        let diff = compute_update_diff(&file, &snapshot, &hv, &hu)
            .expect("expected a diff");
        let any_attr_update = diff
            .values()
            .any(|u| u.a.as_ref().map(|a| !a.is_empty()).unwrap_or(false));
        assert!(
            any_attr_update,
            "expected at least one attribute update in diff: {:?}",
            diff
        );
    }

}

#[cfg(test)]
mod lower_tests {
    use super::*;

    #[test]
    fn infer_percent() { assert_eq!(infer_sensor_format("cpu.usage"), Some(("percent", 0))); }
    #[test]
    fn infer_temperature() { assert_eq!(infer_sensor_format("gpu.temp"), Some(("temperature", 0))); }
    #[test]
    fn infer_clock_raw() { assert_eq!(infer_sensor_format("gpu.clock"), Some(("raw", 0))); }
    #[test]
    fn composite_vram_unsupported() { assert_eq!(infer_sensor_format("gpu.vram"), None); }

    #[test]
    fn text_without_placeholder_returns_none() {
        assert!(lower_text_to_segments("CPU:").is_none());
    }

    #[test]
    fn simple_placeholder_lowers() {
        let segs = lower_text_to_segments("CPU: {cpu.usage}%").unwrap();
        assert_eq!(segs.len(), 3);
        match &segs[0] { TextSegment::Literal(s) => assert_eq!(s, "CPU: "), _ => panic!() }
        match &segs[1] {
            TextSegment::Sensor { path, format, precision } => {
                assert_eq!(path, "cpu.usage");
                assert_eq!(*format, "percent");
                assert_eq!(*precision, 0);
            }
            _ => panic!(),
        }
        match &segs[2] { TextSegment::Literal(s) => assert_eq!(s, "%"), _ => panic!() }
    }

    #[test]
    fn precision_override_applies() {
        let segs = lower_text_to_segments("{gpu.temp(2)}").unwrap();
        match &segs[0] {
            TextSegment::Sensor { precision, .. } => assert_eq!(*precision, 2),
            _ => panic!(),
        }
    }

    #[test]
    fn composite_placeholder_is_not_lowered() {
        // gpu.vram returns None from infer → the whole text should be treated as
        // un-lowered; we signal this by returning None so the old interpolation
        // path handles it.
        assert!(lower_text_to_segments("{gpu.vram}").is_none());
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::omni::history::SensorHistory;
    use crate::omni::types::HtmlNode;
    use omni_shared::SensorSnapshot;
    use std::collections::HashMap;

    #[test]
    fn text_with_sensor_placeholder_emits_span() {
        let node = HtmlNode::Text { content: "CPU: {cpu.usage}%".into() };
        let mut counter = 0;
        let hwinfo_values = HashMap::new();
        let hwinfo_units = HashMap::new();
        let history = SensorHistory::new();
        let mut snap = SensorSnapshot::default();
        snap.cpu.total_usage_percent = 42.0;
        let html = render_initial_node(&node, &snap, &mut counter, &hwinfo_values, &hwinfo_units, &history);
        assert!(html.contains(r#"data-sensor="cpu.usage""#));
        assert!(html.contains(r#"data-sensor-format="percent""#));
        assert!(html.contains(r#"data-sensor-precision="0""#));
        assert!(html.contains(">42%<") || html.contains(">42%</span>"));
    }

    #[test]
    fn text_with_composite_path_falls_back_to_interpolation() {
        let node = HtmlNode::Text { content: "{gpu.vram}".into() };
        let mut counter = 0;
        let hwinfo_values = HashMap::new();
        let hwinfo_units = HashMap::new();
        let history = SensorHistory::new();
        let mut snap = SensorSnapshot::default();
        snap.gpu.vram_used_mb = 4096;
        snap.gpu.vram_total_mb = 12288;
        let html = render_initial_node(&node, &snap, &mut counter, &hwinfo_values, &hwinfo_units, &history);
        assert_eq!(html, "4096/12288");
    }

    #[test]
    fn plain_text_unchanged() {
        let node = HtmlNode::Text { content: "Hello".into() };
        let mut counter = 0;
        let hwinfo_values = HashMap::new();
        let hwinfo_units = HashMap::new();
        let history = SensorHistory::new();
        let snap = SensorSnapshot::default();
        let html = render_initial_node(&node, &snap, &mut counter, &hwinfo_values, &hwinfo_units, &history);
        assert_eq!(html, "Hello");
    }

    fn minimal_omni_file() -> crate::omni::types::OmniFile {
        use crate::omni::types::{OmniFile, Widget};
        OmniFile {
            theme_src: None,
            poll_config: HashMap::new(),
            widgets: vec![Widget {
                id: "test".to_string(),
                name: "Test".to_string(),
                enabled: false,
                template: HtmlNode::Text { content: "".into() },
                style_source: String::new(),
            }],
        }
    }

    #[test]
    fn initial_document_contains_bootstrap_and_trusted_flag() {
        use crate::omni::view_trust::ViewTrust;
        let omni_file = minimal_omni_file();
        let snap = SensorSnapshot::default();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let history = SensorHistory::new();
        let data_dir = std::path::Path::new(".");
        let result = build_initial_html(
            &omni_file,
            &snap,
            800,
            600,
            data_dir,
            "test-overlay",
            &hv,
            &hu,
            &history,
            ViewTrust::LocalAuthored,
        );
        assert!(
            result.full_document.contains("window.__omni_update"),
            "full_document should contain window.__omni_update"
        );
        assert!(
            result.full_document.contains("const TRUSTED = true;"),
            "full_document should contain 'const TRUSTED = true;' for LocalAuthored"
        );
        assert!(
            !result.full_document.contains("function omniUpdate"),
            "full_document must not contain legacy 'function omniUpdate'"
        );
    }

    #[test]
    fn untrusted_document_defangs() {
        use crate::omni::view_trust::ViewTrust;
        let omni_file = minimal_omni_file();
        let snap = SensorSnapshot::default();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let history = SensorHistory::new();
        let data_dir = std::path::Path::new(".");
        let result = build_initial_html(
            &omni_file,
            &snap,
            800,
            600,
            data_dir,
            "test-overlay",
            &hv,
            &hu,
            &history,
            ViewTrust::BundleInstalled,
        );
        assert!(
            result.full_document.contains("const TRUSTED = false;"),
            "full_document should contain 'const TRUSTED = false;' for BundleInstalled"
        );
        assert!(
            result.full_document.contains("eval disabled"),
            "full_document should contain 'eval disabled' defang for untrusted view"
        );
        assert!(
            !result.full_document.contains("function omniUpdate"),
            "full_document must not contain legacy 'function omniUpdate'"
        );
    }

    #[test]
    fn compute_diff_populates_classes_only() {
        use crate::omni::types::{ConditionalClass, HtmlNode, OmniFile, Widget};
        use omni_shared::SensorSnapshot;
        use std::collections::HashMap;

        let mut snap = SensorSnapshot::default();
        snap.cpu.total_usage_percent = 90.0;
        let omni = OmniFile {
            theme_src: None,
            poll_config: Default::default(),
            widgets: vec![Widget {
                id: "w".into(), name: "w".into(), enabled: true,
                template: HtmlNode::Element {
                    tag: "div".into(), id: None, classes: vec!["base".into()],
                    inline_style: None,
                    attributes: vec![],
                    conditional_classes: vec![ConditionalClass {
                        class_name: "sensor-warn".into(),
                        expression: "cpu.usage >= 80".into(),
                    }],
                    children: vec![HtmlNode::Text { content: "{cpu.usage}%".into() }],
                },
                style_source: String::new(),
            }],
        };
        let hv = HashMap::new();
        let hu = HashMap::new();
        let diff = compute_update_diff(&omni, &snap, &hv, &hu).expect("diff");
        let update = diff.values().next().unwrap();
        assert!(update.c.as_ref().unwrap().contains("sensor-warn"));
        assert!(update.t.is_none(), "text should no longer flow through diff");
    }

    #[test]
    fn collect_values_gathers_lowered_paths() {
        use crate::omni::types::{HtmlNode, OmniFile, Widget};
        use omni_shared::SensorSnapshot;
        use std::collections::HashMap;

        let mut snap = SensorSnapshot::default();
        snap.cpu.total_usage_percent = 42.0;
        snap.gpu.temp_c = 60.0;
        let omni = OmniFile {
            theme_src: None,
            poll_config: Default::default(),
            widgets: vec![Widget {
                id: "w".into(), name: "w".into(), enabled: true,
                template: HtmlNode::Element {
                    tag: "div".into(), id: None, classes: vec![], inline_style: None,
                    attributes: vec![],
                    conditional_classes: vec![],
                    children: vec![
                        HtmlNode::Text { content: "{cpu.usage}%".into() },
                        HtmlNode::Text { content: "{gpu.temp}\u{00B0}C".into() },
                    ],
                },
                style_source: String::new(),
            }],
        };
        let hv = HashMap::new();
        let values = collect_sensor_values(&omni, &snap, &hv);
        assert_eq!(values.get("cpu.usage"), Some(&42.0));
        assert_eq!(values.get("gpu.temp"), Some(&60.0));
    }
}
