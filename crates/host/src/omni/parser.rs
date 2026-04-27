//! Parser for .omni file format.
//!
//! A .omni file contains:
//! - Optional `<theme src="..."/>` directive
//! - One or more `<widget id="..." name="..." enabled="true/false">` blocks
//!   - Each widget contains `<template>...</template>` and `<style>...</style>`

use std::collections::{HashMap, HashSet};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use ts_rs::TS;

use super::types::{ConditionalClass, DpiScale, HtmlNode, OmniFile, Widget};
use super::validation;

/// Severity level for parse diagnostics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub enum Severity {
    Error,
    Warning,
}

/// A parse error/warning with position and optional suggestion.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct ParseError {
    pub message: String,
    pub severity: Severity,
    pub line: usize,   // 1-based
    pub column: usize, // 1-based
    pub suggestion: Option<String>,
}

/// Convert a byte offset in a source string to 1-based (line, column).
pub fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let last_newline = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let column = offset - last_newline + 1;
    (line, column)
}

fn make_error(source: &str, offset: usize, message: String) -> ParseError {
    let (line, column) = offset_to_line_col(source, offset);
    ParseError {
        message,
        severity: Severity::Error,
        line,
        column,
        suggestion: None,
    }
}

/// Parse a .omni source string into an OmniFile with full diagnostics (errors + warnings).
///
/// Returns `(Option<OmniFile>, Vec<ParseError>)`. The file is `Some` if parsing succeeded
/// (no fatal errors). The diagnostics vec contains both errors and warnings — warnings
/// don't prevent parsing.
///
/// Pass `hwinfo_connected: true` if HWiNFO shared memory is currently connected, so
/// `hwinfo.*` sensor paths are treated as valid. When `false`, a single warning is emitted
/// for any file that references `hwinfo.*` paths.
pub fn parse_omni_with_diagnostics(source: &str) -> (Option<OmniFile>, Vec<ParseError>) {
    parse_omni_with_diagnostics_inner(source, false)
}

/// Like `parse_omni_with_diagnostics` but takes an explicit `hwinfo_connected` flag.
pub fn parse_omni_with_diagnostics_hwinfo(
    source: &str,
    hwinfo_connected: bool,
) -> (Option<OmniFile>, Vec<ParseError>) {
    parse_omni_with_diagnostics_inner(source, hwinfo_connected)
}

fn parse_omni_with_diagnostics_inner(
    source: &str,
    hwinfo_connected: bool,
) -> (Option<OmniFile>, Vec<ParseError>) {
    let mut errors = Vec::new();
    let mut theme_src = None;
    let mut poll_config = HashMap::new();
    let mut dpi_scale: Option<DpiScale> = None;
    let mut widgets = Vec::new();

    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(true);

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"widget" => match parse_widget(source, &mut reader, e) {
                    Ok(widget) => widgets.push(widget),
                    Err(e) => errors.push(e),
                },
                b"config" => match parse_config_block(source, &mut reader) {
                    Ok(config) => {
                        poll_config = config.poll;
                        dpi_scale = config.dpi_scale;
                    }
                    Err(e) => errors.push(e),
                },
                b"theme" => {
                    theme_src = get_attr(e, "src");
                }
                other => {
                    let name = String::from_utf8_lossy(other).to_string();
                    errors.push(make_error(
                        source,
                        reader.buffer_position() as usize,
                        format!("Unknown top-level element <{}>", name),
                    ));
                }
            },
            Ok(Event::Empty(ref e)) => {
                if e.name().as_ref() == b"theme" {
                    theme_src = get_attr(e, "src");
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                errors.push(make_error(
                    source,
                    reader.buffer_position() as usize,
                    format!("XML parse error: {}", e),
                ));
                break;
            }
            _ => {}
        }
    }

    let has_errors = errors.iter().any(|e| e.severity == Severity::Error);

    if has_errors {
        (None, errors)
    } else {
        let file = OmniFile {
            theme_src,
            poll_config,
            dpi_scale,
            widgets,
        };

        // Run validation on successfully parsed file
        let mut warnings = errors; // may contain warnings from parsing phase
        for widget in &file.widgets {
            // Walk template tree for element names and sensor paths
            validate_template_tree(&widget.template, source, hwinfo_connected, &mut warnings);
        }

        (Some(file), warnings)
    }
}

/// Parse a .omni source string into an OmniFile.
///
/// Backward-compatible wrapper around `parse_omni_with_diagnostics`.
/// Returns `Ok(file)` if no errors (warnings are discarded), `Err(errors)` otherwise.
#[cfg(test)]
pub fn parse_omni(source: &str) -> Result<OmniFile, Vec<ParseError>> {
    let (file, diagnostics) = parse_omni_with_diagnostics(source);
    let errors: Vec<ParseError> = diagnostics
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    if errors.is_empty() {
        Ok(file.unwrap_or_else(OmniFile::empty))
    } else {
        Err(errors)
    }
}

/// Recursively walk an HtmlNode tree to validate element names and sensor paths.
fn validate_template_tree(
    node: &HtmlNode,
    source: &str,
    hwinfo_connected: bool,
    warnings: &mut Vec<ParseError>,
) {
    match node {
        HtmlNode::Element { tag, children, .. } => {
            // Check if element name is known
            if !validation::KNOWN_ELEMENTS.contains(&tag.as_str()) {
                let suggestion = validation::suggest_element(tag);
                let search = format!("<{}", tag);
                let offset = source.find(&search).unwrap_or(0);
                let (line, column) = offset_to_line_col(source, offset);
                warnings.push(ParseError {
                    message: format!("unknown element <{}>", tag),
                    severity: Severity::Error,
                    line,
                    column,
                    suggestion,
                });
            }
            // Recurse into children
            for child in children {
                validate_template_tree(child, source, hwinfo_connected, warnings);
            }
        }
        HtmlNode::Text { content } => {
            // Find approximate offset of this text in source
            let text_offset = source.find(content.as_str()).unwrap_or(0);
            let path_warnings = validation::validate_sensor_paths_with_hwinfo(
                content,
                source,
                text_offset,
                hwinfo_connected,
            );
            warnings.extend(path_warnings);
        }
    }
}

/// Parse a `<widget>` element and its children (`<template>` and `<style>`).
fn parse_widget(
    source: &str,
    reader: &mut Reader<&[u8]>,
    start: &BytesStart,
) -> Result<Widget, ParseError> {
    let id = get_attr(start, "id").ok_or_else(|| {
        make_error(
            source,
            reader.buffer_position() as usize,
            "Widget missing required 'id' attribute".to_string(),
        )
    })?;

    let name = get_attr(start, "name").ok_or_else(|| {
        make_error(
            source,
            reader.buffer_position() as usize,
            format!("Widget '{}' missing required 'name' attribute", id),
        )
    })?;

    let enabled = get_attr(start, "enabled")
        .map(|v| v != "false")
        .unwrap_or(true);

    let mut template: Option<HtmlNode> = None;
    let mut style_source = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"template" => {
                    template = Some(parse_template_children(source, reader)?);
                }
                b"style" => {
                    style_source = read_text_content(source, reader, "style")?;
                }
                _ => {}
            },
            Ok(Event::End(ref e)) if e.name().as_ref() == b"widget" => break,
            Ok(Event::Eof) => {
                return Err(make_error(
                    source,
                    reader.buffer_position() as usize,
                    format!("Unexpected EOF inside widget '{}'", id),
                ));
            }
            Err(e) => {
                return Err(make_error(
                    source,
                    reader.buffer_position() as usize,
                    format!("XML error in widget '{}': {}", id, e),
                ));
            }
            _ => {}
        }
    }

    let template = template.unwrap_or(HtmlNode::Element {
        tag: "div".to_string(),
        id: None,
        classes: vec![],
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![],
        children: vec![],
    });

    Ok(Widget {
        id,
        name,
        enabled,
        template,
        style_source,
    })
}

/// Parse the children of a `<template>` element into an HtmlNode tree.
/// Wraps multiple root elements in a synthetic `<div>`.
fn parse_template_children(
    source: &str,
    reader: &mut Reader<&[u8]>,
) -> Result<HtmlNode, ParseError> {
    let mut children = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let node = parse_html_element(source, reader, e)?;
                children.push(node);
            }
            Ok(Event::Empty(ref e)) => {
                let pos = reader.buffer_position() as usize;
                let node = parse_empty_html_element(source, pos, e)?;
                children.push(node);
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if !text.trim().is_empty() {
                    children.push(HtmlNode::Text { content: text });
                }
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"template" => break,
            Ok(Event::Eof) => {
                return Err(make_error(
                    source,
                    reader.buffer_position() as usize,
                    "Unexpected EOF inside <template>".to_string(),
                ));
            }
            Err(e) => {
                return Err(make_error(
                    source,
                    reader.buffer_position() as usize,
                    format!("XML error in template: {}", e),
                ));
            }
            _ => {}
        }
    }

    // If there's exactly one child, use it as the root. Otherwise, wrap in a div.
    if children.len() == 1 {
        Ok(children.remove(0))
    } else {
        Ok(HtmlNode::Element {
            tag: "div".to_string(),
            id: None,
            classes: vec![],
            inline_style: None,
            conditional_classes: vec![],
            attributes: vec![],
            children,
        })
    }
}

/// Parse an HTML element with children.
fn parse_html_element(
    source: &str,
    reader: &mut Reader<&[u8]>,
    start: &BytesStart,
) -> Result<HtmlNode, ParseError> {
    let tag = String::from_utf8_lossy(start.name().as_ref()).to_string();

    // Intercept <chart> before doing normal element construction. Desugar into
    // an SVG subtree at parse time so downstream pipeline stages never see a
    // raw <chart> element.
    if tag == "chart" {
        let chart_attrs = collect_chart_attrs(start);
        let pos = reader.buffer_position() as usize;
        let node = desugar_chart(&chart_attrs, source, pos)?;
        // Consume any inner content until the matching </chart> end tag so the
        // event stream stays balanced. Charts don't have meaningful children,
        // so we discard whatever's inside.
        loop {
            match reader.read_event() {
                Ok(Event::End(ref e)) if e.name().as_ref() == b"chart" => break,
                Ok(Event::Eof) => {
                    return Err(make_error(
                        source,
                        reader.buffer_position() as usize,
                        "Unexpected EOF inside <chart>".to_string(),
                    ));
                }
                Err(e) => {
                    return Err(make_error(
                        source,
                        reader.buffer_position() as usize,
                        format!("XML error in <chart>: {}", e),
                    ));
                }
                _ => {}
            }
        }
        return Ok(node);
    }

    // Intercept <chart-card> the same way: desugar at parse time into a fully
    // laid-out SVG subtree with title, axis labels, and an inner chart.
    if tag == "chart-card" {
        let chart_attrs = collect_chart_attrs(start);
        tracing::debug!(
            attrs = ?chart_attrs,
            "intercepting <chart-card> (non-self-closing) for desugaring"
        );
        let pos = reader.buffer_position() as usize;
        let node = desugar_chart_card(&chart_attrs, source, pos)?;
        loop {
            match reader.read_event() {
                Ok(Event::End(ref e)) if e.name().as_ref() == b"chart-card" => break,
                Ok(Event::Eof) => {
                    return Err(make_error(
                        source,
                        reader.buffer_position() as usize,
                        "Unexpected EOF inside <chart-card>".to_string(),
                    ));
                }
                Err(e) => {
                    return Err(make_error(
                        source,
                        reader.buffer_position() as usize,
                        format!("XML error in <chart-card>: {}", e),
                    ));
                }
                _ => {}
            }
        }
        return Ok(node);
    }

    let id = get_attr(start, "id");
    let classes = get_attr(start, "class")
        .map(|c| c.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let inline_style = get_attr(start, "style");
    let conditional_classes = get_conditional_classes(start);
    let attributes = get_extra_attributes(start);

    let mut children = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let child = parse_html_element(source, reader, e)?;
                children.push(child);
            }
            Ok(Event::Empty(ref e)) => {
                let pos = reader.buffer_position() as usize;
                children.push(parse_empty_html_element(source, pos, e)?);
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if !text.trim().is_empty() {
                    children.push(HtmlNode::Text { content: text });
                }
            }
            Ok(Event::End(ref e)) => {
                let end_tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if end_tag == tag {
                    break;
                }
            }
            Ok(Event::Eof) => {
                return Err(make_error(
                    source,
                    reader.buffer_position() as usize,
                    format!("Unexpected EOF inside <{}>", tag),
                ));
            }
            Err(e) => {
                return Err(make_error(
                    source,
                    reader.buffer_position() as usize,
                    format!("XML error in <{}>: {}", tag, e),
                ));
            }
            _ => {}
        }
    }

    Ok(HtmlNode::Element {
        tag,
        id,
        classes,
        inline_style,
        conditional_classes,
        attributes,
        children,
    })
}

/// Parse a self-closing HTML element (e.g., `<br/>`, `<spacer/>`).
fn parse_empty_html_element(
    source: &str,
    pos: usize,
    start: &BytesStart,
) -> Result<HtmlNode, ParseError> {
    let tag = String::from_utf8_lossy(start.name().as_ref()).to_string();

    // Intercept <chart/> and desugar at parse time.
    if tag == "chart" {
        let chart_attrs = collect_chart_attrs(start);
        return desugar_chart(&chart_attrs, source, pos);
    }

    // Intercept <chart-card/> self-closing and desugar at parse time.
    if tag == "chart-card" {
        let chart_attrs = collect_chart_attrs(start);
        tracing::debug!(
            attrs = ?chart_attrs,
            "intercepting <chart-card/> (self-closing) for desugaring"
        );
        return desugar_chart_card(&chart_attrs, source, pos);
    }

    let id = get_attr(start, "id");
    let classes = get_attr(start, "class")
        .map(|c| c.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let inline_style = get_attr(start, "style");
    let conditional_classes = get_conditional_classes(start);
    let attributes = get_extra_attributes(start);

    Ok(HtmlNode::Element {
        tag,
        id,
        classes,
        inline_style,
        conditional_classes,
        attributes,
        children: vec![],
    })
}

/// Read all text content until the closing tag is found.
fn read_text_content(
    source: &str,
    reader: &mut Reader<&[u8]>,
    tag: &str,
) -> Result<String, ParseError> {
    let mut content = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Text(ref e)) => {
                content.push_str(&e.unescape().unwrap_or_default());
            }
            Ok(Event::CData(ref e)) => {
                // CData content is not escaped, convert bytes directly.
                content.push_str(&String::from_utf8_lossy(e));
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == tag.as_bytes() {
                    break;
                }
            }
            Ok(Event::Eof) => {
                return Err(make_error(
                    source,
                    reader.buffer_position() as usize,
                    format!("Unexpected EOF inside <{}>", tag),
                ));
            }
            Err(e) => {
                return Err(make_error(
                    source,
                    reader.buffer_position() as usize,
                    format!("XML error reading <{}>: {}", tag, e),
                ));
            }
            _ => {}
        }
    }

    Ok(content)
}

/// Result of parsing the `<config>` block. Carries the per-sensor poll map
/// AND the optional per-overlay DPI scale directive.
struct ConfigBlock {
    poll: HashMap<String, u64>,
    dpi_scale: Option<DpiScale>,
}

/// Parse a `<config>` block containing `<poll sensor=".." interval=".."/>` and/or
/// `<dpi-scale value="auto|<float>"/>` entries.
fn parse_config_block(source: &str, reader: &mut Reader<&[u8]>) -> Result<ConfigBlock, ParseError> {
    let mut poll = HashMap::new();
    let mut dpi_scale: Option<DpiScale> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"poll" => {
                let sensor = get_attr(e, "sensor");
                let interval = get_attr(e, "interval").and_then(|v| v.parse::<u64>().ok());
                if let (Some(sensor), Some(interval)) = (sensor, interval) {
                    poll.insert(sensor, interval);
                }
            }
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"dpi-scale" => {
                if dpi_scale.is_some() {
                    return Err(make_error(
                        source,
                        reader.buffer_position() as usize,
                        "duplicate <dpi-scale> in <config>".to_string(),
                    ));
                }
                let value_str = get_attr(e, "value").ok_or_else(|| {
                    let mut err = make_error(
                        source,
                        reader.buffer_position() as usize,
                        "<dpi-scale> requires a 'value' attribute".to_string(),
                    );
                    err.suggestion =
                        Some("expected `value=\"auto\"` or `value=\"<number>\"`".to_string());
                    err
                })?;
                dpi_scale = Some(parse_dpi_scale_value(
                    &value_str,
                    source,
                    reader.buffer_position() as usize,
                )?);
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"config" => break,
            Ok(Event::Eof) => {
                return Err(make_error(
                    source,
                    reader.buffer_position() as usize,
                    "Unexpected EOF inside <config>".to_string(),
                ));
            }
            _ => {}
        }
    }

    Ok(ConfigBlock { poll, dpi_scale })
}

/// Parse the `value` attribute of `<dpi-scale>`. Accepts "auto" (case-
/// insensitive) or a finite float in [0.5, 4.0].
fn parse_dpi_scale_value(s: &str, source: &str, offset: usize) -> Result<DpiScale, ParseError> {
    if s.eq_ignore_ascii_case("auto") {
        return Ok(DpiScale::Auto);
    }
    let n: f64 = s.parse().map_err(|_| {
        let mut err = make_error(
            source,
            offset,
            format!("<dpi-scale value=\"{s}\"> is not a number or 'auto'"),
        );
        err.suggestion = Some("expected `auto` or a number between 0.5 and 4.0".to_string());
        err
    })?;
    if !n.is_finite() || !(0.5..=4.0).contains(&n) {
        let mut err = make_error(
            source,
            offset,
            format!("<dpi-scale value=\"{n}\"> is out of range"),
        );
        err.suggestion = Some("must be between 0.5 and 4.0".to_string());
        return Err(err);
    }
    Ok(DpiScale::Manual(n))
}

/// Extract an attribute value from an XML element.
fn get_attr(start: &BytesStart, name: &str) -> Option<String> {
    start
        .attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == name.as_bytes())
        .map(|a| String::from_utf8_lossy(&a.value).to_string())
}

/// Extract arbitrary attributes that aren't handled by dedicated fields
/// (i.e., not `id`, `class`, `style`, or `class:NAME`). Preserves source
/// ordering so diffs remain deterministic.
fn get_extra_attributes(start: &BytesStart) -> Vec<(String, String)> {
    let class_prefix = b"class:";
    start
        .attributes()
        .filter_map(|a| a.ok())
        .filter_map(|a| {
            let key = a.key.as_ref();
            if key == b"id" || key == b"class" || key == b"style" || key.starts_with(class_prefix) {
                return None;
            }
            let name = String::from_utf8_lossy(key).to_string();
            let value = String::from_utf8_lossy(&a.value).to_string();
            Some((name, value))
        })
        .collect()
}

/// Extract conditional class bindings from `class:name="expression"` attributes.
fn get_conditional_classes(start: &BytesStart) -> Vec<ConditionalClass> {
    let prefix = b"class:";
    start
        .attributes()
        .filter_map(|a| a.ok())
        .filter_map(|a| {
            let key = a.key.as_ref();
            if key.starts_with(prefix) && key.len() > prefix.len() {
                let class_name = String::from_utf8_lossy(&key[prefix.len()..]).to_string();
                let expression = String::from_utf8_lossy(&a.value).to_string();
                Some(ConditionalClass {
                    class_name,
                    expression,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Collect all attributes on a `<chart>` element into a map for desugaring.
fn collect_chart_attrs(start: &BytesStart) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for attr in start.attributes().flatten() {
        let k = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let v = String::from_utf8_lossy(&attr.value).to_string();
        out.insert(k, v);
    }
    out
}

/// Desugar a `<chart>` element into an SVG subtree at parse time.
/// Dispatched from `parse_html_element` / `parse_empty_html_element` when
/// the tag name is "chart".
fn desugar_chart(
    attrs: &HashMap<String, String>,
    source: &str,
    pos: usize,
) -> Result<HtmlNode, ParseError> {
    let chart_type = attrs.get("type").ok_or_else(|| {
        make_error(
            source,
            pos,
            "chart element requires 'type' attribute".to_string(),
        )
    })?;

    match chart_type.as_str() {
        "line" => desugar_chart_line(attrs, source, pos),
        "bar" => desugar_chart_bar(attrs, source, pos),
        "pie" => desugar_chart_pie(attrs, source, pos),
        other => Err(make_error(
            source,
            pos,
            format!("unknown chart type: {}", other),
        )),
    }
}

/// Desugar a `<chart-card>` element into a full SVG layout with title,
/// Y-axis tick labels, X-axis labels, and a nested chart body. The inner
/// chart is delegated to `desugar_chart` with overridden width/height that
/// match the plot area.
fn desugar_chart_card(
    attrs: &HashMap<String, String>,
    source: &str,
    pos: usize,
) -> Result<HtmlNode, ParseError> {
    let chart_type = attrs.get("type").ok_or_else(|| {
        make_error(
            source,
            pos,
            "chart-card requires 'type' attribute".to_string(),
        )
    })?;
    let unit = attrs.get("unit").map(String::as_str).unwrap_or("none");
    let title = attrs.get("title").cloned();
    let y_ticks: usize = attrs
        .get("y-ticks")
        .and_then(|s| s.parse().ok())
        .unwrap_or(4);

    // Layout constants for v1. Default CSS font sizes: 10px title, 8px labels.
    // Leaves ~8px padding between plot bottom (94) and x-labels (106).
    let (card_w, card_h) = (260i32, 110i32);
    let (plot_x, plot_y, plot_w, plot_h) = (48i32, 22i32, 200i32, 72i32);

    let mut children: Vec<HtmlNode> = Vec::new();

    // Title
    if let Some(title_text) = title {
        children.push(HtmlNode::Element {
            tag: "text".to_string(),
            id: None,
            classes: vec!["omni-chart-card-title".to_string()],
            inline_style: None,
            conditional_classes: vec![],
            attributes: vec![
                ("x".to_string(), format!("{}", card_w / 2)),
                ("y".to_string(), "14".to_string()),
                ("text-anchor".to_string(), "middle".to_string()),
            ],
            children: vec![HtmlNode::Text {
                content: title_text,
            }],
        });
    }

    // Y-axis labels (only for line/bar)
    let sensor_for_ticks = match chart_type.as_str() {
        "line" | "bar" => attrs.get("sensor").cloned(),
        _ => None,
    };
    if let Some(sensor) = sensor_for_ticks.as_ref() {
        let step_y = plot_h as f64 / (y_ticks.saturating_sub(1).max(1)) as f64;
        for i in 0..y_ticks {
            // Reverse so index 0 is at the bottom (top tick is the highest value)
            let display_index = y_ticks - 1 - i;
            let y = plot_y as f64 + (i as f64 * step_y) + 4.0;
            let label_expr = format!(
                "{{nice_tick({}, {}, {}, {})}}",
                sensor, unit, display_index, y_ticks
            );
            children.push(HtmlNode::Element {
                tag: "text".to_string(),
                id: None,
                classes: vec!["omni-chart-card-y-label".to_string()],
                inline_style: None,
                conditional_classes: vec![],
                attributes: vec![
                    ("x".to_string(), format!("{}", plot_x - 4)),
                    ("y".to_string(), format!("{:.0}", y)),
                    ("text-anchor".to_string(), "end".to_string()),
                ],
                children: vec![HtmlNode::Text {
                    content: label_expr,
                }],
            });
        }
    }

    // Inner chart. We intentionally DO NOT wrap the inner chart in a
    // nested <svg> — a nested SVG without explicit width/height defaults
    // to 100% × 100% of the parent viewport per SVG 1.1, which would
    // cause the polyline/rects to stretch across the entire card area
    // and overlap the title / axis labels. Instead, extract the shape
    // children from desugar_chart's SVG and wrap them in a <g transform>
    // positioned at the plot area. Polyline points and rect coordinates
    // are already in the plot_w × plot_h user-unit space.
    let mut inner_attrs = attrs.clone();
    inner_attrs.insert("width".to_string(), plot_w.to_string());
    inner_attrs.insert("height".to_string(), plot_h.to_string());
    let inner_chart_svg = desugar_chart(&inner_attrs, source, pos)?;
    let inner_shapes = match inner_chart_svg {
        HtmlNode::Element { children, .. } => children,
        other => vec![other],
    };
    children.push(HtmlNode::Element {
        tag: "g".to_string(),
        id: None,
        classes: vec![format!("omni-chart-card-plot")],
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![(
            "transform".to_string(),
            format!("translate({},{})", plot_x, plot_y),
        )],
        children: inner_shapes,
    });

    // X-axis labels (only for line charts)
    if chart_type == "line" {
        children.push(HtmlNode::Element {
            tag: "text".to_string(),
            id: None,
            classes: vec!["omni-chart-card-x-label".to_string()],
            inline_style: None,
            conditional_classes: vec![],
            attributes: vec![
                ("x".to_string(), format!("{}", plot_x)),
                ("y".to_string(), format!("{}", card_h - 4)),
            ],
            children: vec![HtmlNode::Text {
                content: "60s ago".to_string(),
            }],
        });
        children.push(HtmlNode::Element {
            tag: "text".to_string(),
            id: None,
            classes: vec!["omni-chart-card-x-label".to_string()],
            inline_style: None,
            conditional_classes: vec![],
            attributes: vec![
                ("x".to_string(), format!("{}", plot_x + plot_w)),
                ("y".to_string(), format!("{}", card_h - 4)),
                ("text-anchor".to_string(), "end".to_string()),
            ],
            children: vec![HtmlNode::Text {
                content: "now".to_string(),
            }],
        });
    }

    let classes = vec![
        "omni-chart-card".to_string(),
        format!("omni-chart-card-{}", chart_type),
    ];

    Ok(HtmlNode::Element {
        tag: "svg".to_string(),
        id: None,
        classes,
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![("viewBox".to_string(), format!("0 0 {} {}", card_w, card_h))],
        children,
    })
}

fn desugar_chart_line(
    attrs: &HashMap<String, String>,
    source: &str,
    pos: usize,
) -> Result<HtmlNode, ParseError> {
    let sensor = attrs.get("sensor").ok_or_else(|| {
        make_error(
            source,
            pos,
            "line chart requires 'sensor' attribute".to_string(),
        )
    })?;
    let width = attrs.get("width").map(String::as_str).unwrap_or("200");
    let height = attrs.get("height").map(String::as_str).unwrap_or("60");
    let stroke = attrs
        .get("stroke")
        .map(String::as_str)
        .unwrap_or("currentColor");
    let stroke_width = attrs.get("stroke-width").map(String::as_str).unwrap_or("2");
    let fill = attrs.get("fill").map(String::as_str).unwrap_or("none");
    let extra_class = attrs.get("class").map(String::as_str).unwrap_or("");

    // Build the points interpolation. Use fixed-scale if min/max were given,
    // otherwise auto-scale.
    let points_expr = match (attrs.get("min"), attrs.get("max")) {
        (Some(min), Some(max)) => format!(
            "{{chart_polyline({}, {}, {}, {}, {})}}",
            sensor, width, height, min, max
        ),
        _ => format!("{{chart_polyline({}, {}, {})}}", sensor, width, height),
    };

    let mut svg_classes = vec!["omni-chart".to_string(), "omni-chart-line".to_string()];
    for c in extra_class.split_whitespace() {
        svg_classes.push(c.to_string());
    }

    let polyline = HtmlNode::Element {
        tag: "polyline".to_string(),
        id: None,
        classes: vec!["omni-chart-line-stroke".to_string()],
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![
            ("fill".to_string(), fill.to_string()),
            ("stroke".to_string(), stroke.to_string()),
            ("stroke-width".to_string(), stroke_width.to_string()),
            ("points".to_string(), points_expr),
        ],
        children: vec![],
    };

    Ok(HtmlNode::Element {
        tag: "svg".to_string(),
        id: None,
        classes: svg_classes,
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![
            ("viewBox".to_string(), format!("0 0 {} {}", width, height)),
            ("preserveAspectRatio".to_string(), "none".to_string()),
        ],
        children: vec![polyline],
    })
}

fn desugar_chart_bar(
    attrs: &HashMap<String, String>,
    source: &str,
    pos: usize,
) -> Result<HtmlNode, ParseError> {
    let sensor = attrs.get("sensor").ok_or_else(|| {
        make_error(
            source,
            pos,
            "bar chart requires 'sensor' attribute".to_string(),
        )
    })?;
    let width = attrs.get("width").map(String::as_str).unwrap_or("100");
    let height = attrs.get("height").map(String::as_str).unwrap_or("50");
    let min = attrs.get("min").map(String::as_str).unwrap_or("0");
    let max = attrs.get("max").map(String::as_str).unwrap_or("100");
    let fill = attrs
        .get("fill")
        .map(String::as_str)
        .unwrap_or("currentColor");
    let extra_class = attrs.get("class").map(String::as_str).unwrap_or("");

    let mut svg_classes = vec!["omni-chart".to_string(), "omni-chart-bar".to_string()];
    for c in extra_class.split_whitespace() {
        svg_classes.push(c.to_string());
    }

    let track = HtmlNode::Element {
        tag: "rect".to_string(),
        id: None,
        classes: vec!["omni-chart-bar-track".to_string()],
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![
            ("x".to_string(), "0".to_string()),
            ("y".to_string(), "0".to_string()),
            ("width".to_string(), width.to_string()),
            ("height".to_string(), height.to_string()),
        ],
        children: vec![],
    };

    let fill_rect = HtmlNode::Element {
        tag: "rect".to_string(),
        id: None,
        classes: vec!["omni-chart-bar-fill".to_string()],
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![
            ("fill".to_string(), fill.to_string()),
            ("x".to_string(), "0".to_string()),
            (
                "y".to_string(),
                format!("{{bar_y({}, {}, {}, {})}}", sensor, height, min, max),
            ),
            ("width".to_string(), width.to_string()),
            (
                "height".to_string(),
                format!("{{bar_height({}, {}, {}, {})}}", sensor, height, min, max),
            ),
        ],
        children: vec![],
    };

    Ok(HtmlNode::Element {
        tag: "svg".to_string(),
        id: None,
        classes: svg_classes,
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![
            ("viewBox".to_string(), format!("0 0 {} {}", width, height)),
            ("preserveAspectRatio".to_string(), "none".to_string()),
        ],
        children: vec![track, fill_rect],
    })
}

fn desugar_chart_pie(
    attrs: &HashMap<String, String>,
    source: &str,
    pos: usize,
) -> Result<HtmlNode, ParseError> {
    let value = attrs.get("value").ok_or_else(|| {
        make_error(
            source,
            pos,
            "pie chart requires 'value' attribute".to_string(),
        )
    })?;
    let total = attrs.get("total").ok_or_else(|| {
        make_error(
            source,
            pos,
            "pie chart requires 'total' attribute".to_string(),
        )
    })?;
    let radius = attrs.get("radius").map(String::as_str).unwrap_or("40");
    let stroke_width = attrs
        .get("stroke-width")
        .map(String::as_str)
        .unwrap_or("10");
    let stroke = attrs
        .get("stroke")
        .map(String::as_str)
        .unwrap_or("currentColor");
    let extra_class = attrs.get("class").map(String::as_str).unwrap_or("");

    let mut svg_classes = vec!["omni-chart".to_string(), "omni-chart-pie".to_string()];
    for c in extra_class.split_whitespace() {
        svg_classes.push(c.to_string());
    }

    let track = HtmlNode::Element {
        tag: "circle".to_string(),
        id: None,
        classes: vec!["omni-chart-pie-track".to_string()],
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![
            ("cx".to_string(), "50".to_string()),
            ("cy".to_string(), "50".to_string()),
            ("r".to_string(), radius.to_string()),
            ("fill".to_string(), "none".to_string()),
            ("stroke".to_string(), "rgba(255,255,255,0.1)".to_string()),
            ("stroke-width".to_string(), stroke_width.to_string()),
        ],
        children: vec![],
    };

    let fill = HtmlNode::Element {
        tag: "circle".to_string(),
        id: None,
        classes: vec!["omni-chart-pie-fill".to_string()],
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![
            ("cx".to_string(), "50".to_string()),
            ("cy".to_string(), "50".to_string()),
            ("r".to_string(), radius.to_string()),
            ("fill".to_string(), "none".to_string()),
            ("stroke".to_string(), stroke.to_string()),
            ("stroke-width".to_string(), stroke_width.to_string()),
            (
                "stroke-dasharray".to_string(),
                format!("{{circumference({})}}", radius),
            ),
            (
                "stroke-dashoffset".to_string(),
                format!("{{ratio_dashoffset({}, {}, {})}}", value, total, radius),
            ),
            ("transform".to_string(), "rotate(-90 50 50)".to_string()),
        ],
        children: vec![],
    };

    Ok(HtmlNode::Element {
        tag: "svg".to_string(),
        id: None,
        classes: svg_classes,
        inline_style: None,
        conditional_classes: vec![],
        attributes: vec![("viewBox".to_string(), "0 0 100 100".to_string())],
        children: vec![track, fill],
    })
}

/// Walk all widgets in an OmniFile and return the set of sensor paths
/// that are referenced inside chart helper function calls in element
/// attributes. Used by the host to decide which sensors need a history buffer.
pub fn collect_chart_sensors(file: &OmniFile) -> HashSet<String> {
    let mut sensors = HashSet::new();
    for widget in &file.widgets {
        if !widget.enabled {
            continue;
        }
        walk_for_chart_sensors(&widget.template, &mut sensors);
    }
    sensors
}

fn walk_for_chart_sensors(node: &HtmlNode, sensors: &mut HashSet<String>) {
    if let HtmlNode::Element {
        attributes,
        children,
        ..
    } = node
    {
        for (_, value) in attributes {
            extract_chart_sensor_args(value, sensors);
        }
        for child in children {
            walk_for_chart_sensors(child, sensors);
        }
    }
}

/// Scan an attribute value for `{function(sensor, ...)}` calls and extract
/// the sensor-path arguments.
fn extract_chart_sensor_args(value: &str, sensors: &mut HashSet<String>) {
    let mut rest = value;
    while let Some(open) = rest.find('{') {
        let body_start = open + 1;
        let close = match rest[body_start..].find('}') {
            Some(c) => body_start + c,
            None => break,
        };
        let body = &rest[body_start..close];
        extract_sensors_from_expression(body, sensors);
        rest = &rest[close + 1..];
    }
}

fn extract_sensors_from_expression(expr: &str, sensors: &mut HashSet<String>) {
    let paren_start = match expr.find('(') {
        Some(p) => p,
        None => return,
    };
    if !expr.ends_with(')') {
        return;
    }
    let name = expr[..paren_start].trim();
    let args_str = &expr[paren_start + 1..expr.len() - 1];
    let args: Vec<&str> = args_str.split(',').map(str::trim).collect();

    // Functions where the first arg is a sensor path
    let first_arg_sensor = matches!(
        name,
        "chart_polyline"
            | "chart_path"
            | "bar_height"
            | "bar_y"
            | "buffer_min"
            | "buffer_max"
            | "buffer_avg"
            | "nice_min"
            | "nice_max"
            | "nice_tick"
            | "format_value"
    );
    if first_arg_sensor {
        if let Some(first) = args.first() {
            if first.contains('.') && first.parse::<f64>().is_err() {
                sensors.insert(first.to_string());
            }
        }
    }
    // ratio_dashoffset takes two sensor args
    if name == "ratio_dashoffset" && args.len() >= 2 {
        for arg in &args[..2] {
            if arg.contains('.') && arg.parse::<f64>().is_err() {
                sensors.insert(arg.to_string());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_omni() {
        let source = r#"
            <widget id="test" name="Test Widget" enabled="true">
                <template>
                    <div class="panel">
                        <span>Hello</span>
                    </div>
                </template>
                <style>
                    .panel { color: white; }
                </style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        assert_eq!(file.widgets.len(), 1);
        assert_eq!(file.widgets[0].id, "test");
        assert_eq!(file.widgets[0].name, "Test Widget");
        assert!(file.widgets[0].enabled);
        assert!(file.widgets[0].style_source.contains("color: white"));
    }

    #[test]
    fn parse_with_theme() {
        let source = r#"
            <theme src="./themes/dark.css" />
            <widget id="fps" name="FPS" enabled="true">
                <template><span>{fps}</span></template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        assert_eq!(file.theme_src, Some("./themes/dark.css".to_string()));
    }

    #[test]
    fn parse_multiple_widgets() {
        let source = r#"
            <widget id="cpu" name="CPU Monitor" enabled="true">
                <template><div><span>CPU</span></div></template>
                <style></style>
            </widget>
            <widget id="gpu" name="GPU Monitor" enabled="false">
                <template><div><span>GPU</span></div></template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        assert_eq!(file.widgets.len(), 2);
        assert_eq!(file.widgets[0].id, "cpu");
        assert!(file.widgets[0].enabled);
        assert_eq!(file.widgets[1].id, "gpu");
        assert!(!file.widgets[1].enabled);
    }

    #[test]
    fn parse_inline_styles_and_ids() {
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template>
                    <div id="main" class="panel dark" style="position: fixed; top: 10px;">
                        <span class="value">text</span>
                    </div>
                </template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        if let HtmlNode::Element {
            id,
            classes,
            inline_style,
            children,
            ..
        } = &file.widgets[0].template
        {
            assert_eq!(id.as_deref(), Some("main"));
            assert_eq!(classes, &["panel", "dark"]);
            assert!(inline_style.as_ref().unwrap().contains("position: fixed"));
            assert_eq!(children.len(), 1);
        } else {
            panic!("Expected Element");
        }
    }

    #[test]
    fn parse_sensor_interpolation_in_text() {
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template>
                    <span>CPU: {cpu.usage}%</span>
                </template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        if let HtmlNode::Element { children, .. } = &file.widgets[0].template {
            if let HtmlNode::Text { content } = &children[0] {
                assert_eq!(content, "CPU: {cpu.usage}%");
            } else {
                panic!("Expected Text node");
            }
        }
    }

    #[test]
    fn parse_config_block() {
        let source = r#"
            <config>
                <poll sensor="fps" interval="100" />
                <poll sensor="gpu.temp" interval="250" />
                <poll sensor="cpu.usage" interval="1000" />
            </config>
            <widget id="test" name="Test" enabled="true">
                <template><span>test</span></template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        assert_eq!(file.poll_config.len(), 3);
        assert_eq!(file.poll_config.get("fps"), Some(&100));
        assert_eq!(file.poll_config.get("gpu.temp"), Some(&250));
        assert_eq!(file.poll_config.get("cpu.usage"), Some(&1000));
    }

    #[test]
    fn omni_file_without_config_has_empty_poll_config() {
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template><span>test</span></template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        assert!(file.poll_config.is_empty());
    }

    #[test]
    fn missing_widget_id_returns_error() {
        let source = r#"
            <widget name="Test" enabled="true">
                <template><div></div></template>
                <style></style>
            </widget>
        "#;

        let result = parse_omni(source);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert_eq!(errors[0].severity, Severity::Error);
        assert!(errors[0].message.contains("missing required 'id'"));
        assert!(errors[0].line > 0);
        assert!(errors[0].column > 0);
    }

    #[test]
    fn parse_conditional_class_bindings() {
        let source = r#"
            <widget id="gpu" name="GPU Monitor" enabled="true">
                <template>
                    <div class="panel" class:warning="gpu.temp > 80" class:critical="gpu.temp > 95">
                        <span>{gpu.temp}°C</span>
                    </div>
                </template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        if let HtmlNode::Element {
            classes,
            conditional_classes,
            ..
        } = &file.widgets[0].template
        {
            assert_eq!(classes, &["panel"]);
            assert_eq!(conditional_classes.len(), 2);

            assert_eq!(conditional_classes[0].class_name, "warning");
            assert_eq!(conditional_classes[0].expression, "gpu.temp > 80");

            assert_eq!(conditional_classes[1].class_name, "critical");
            assert_eq!(conditional_classes[1].expression, "gpu.temp > 95");
        } else {
            panic!("Expected Element");
        }
    }

    #[test]
    fn parse_chart_line_desugars_to_svg_polyline() {
        let source = r#"<widget id="fps" name="FPS">
<template>
  <chart type="line" sensor="cpu.usage"/>
</template>
<style></style>
</widget>"#;
        let (file, _diagnostics) = parse_omni_with_diagnostics_hwinfo(source, false);
        let file = file.expect("parse succeeded");
        let widget = &file.widgets[0];
        // The template root's first element child should be an <svg>
        // containing a <polyline> whose points attribute has a chart_polyline call.
        let svg = match &widget.template {
            crate::omni::types::HtmlNode::Element { tag, children, .. } => {
                if tag == "svg" {
                    &widget.template
                } else {
                    children
                        .iter()
                        .find(|c| matches!(c, crate::omni::types::HtmlNode::Element { .. }))
                        .expect("template should have an element child")
                }
            }
            _ => panic!("template should be Element"),
        };
        match svg {
            crate::omni::types::HtmlNode::Element {
                tag,
                children,
                classes,
                attributes,
                ..
            } => {
                assert_eq!(tag, "svg");
                assert!(
                    classes.iter().any(|c| c == "omni-chart-line"),
                    "svg should have omni-chart-line class, got {:?}",
                    classes
                );
                // Should have a viewBox attribute
                assert!(
                    attributes.iter().any(|(k, _)| k == "viewBox"),
                    "svg should have viewBox attribute, got {:?}",
                    attributes
                );
                // Child should be a polyline with interpolated points
                let polyline = children
                    .iter()
                    .find(|c| matches!(c, crate::omni::types::HtmlNode::Element { tag, .. } if tag == "polyline"))
                    .expect("svg should contain a polyline child");
                match polyline {
                    crate::omni::types::HtmlNode::Element { attributes, .. } => {
                        let points_attr = attributes
                            .iter()
                            .find(|(k, _)| k == "points")
                            .expect("polyline should have points");
                        assert!(
                            points_attr.1.contains("chart_polyline"),
                            "points should contain chart_polyline call: {}",
                            points_attr.1
                        );
                        assert!(points_attr.1.contains("cpu.usage"));
                    }
                    _ => panic!("polyline child should be Element"),
                }
            }
            _ => panic!("expected svg element, got {:?}", svg),
        }
    }

    #[test]
    fn offset_to_line_col_basic() {
        let source = "line1\nline2\nline3";
        assert_eq!(offset_to_line_col(source, 0), (1, 1)); // start of line1
        assert_eq!(offset_to_line_col(source, 5), (1, 6)); // newline at end of line1
        assert_eq!(offset_to_line_col(source, 6), (2, 1)); // start of line2
        assert_eq!(offset_to_line_col(source, 12), (3, 1)); // start of line3
    }

    #[test]
    fn offset_to_line_col_beyond_end() {
        let source = "abc";
        assert_eq!(offset_to_line_col(source, 100), (1, 4)); // clamped
    }

    #[test]
    fn parse_chart_bar_desugars_to_svg_rects() {
        let source = r#"<widget id="cpu" name="CPU">
<template>
  <chart type="bar" sensor="cpu.usage" min="0" max="100"/>
</template>
<style></style>
</widget>"#;
        let (file, _diag) = parse_omni_with_diagnostics_hwinfo(source, false);
        let file = file.expect("parse succeeded");
        let widget = &file.widgets[0];
        let svg = match &widget.template {
            crate::omni::types::HtmlNode::Element { tag, children, .. } => {
                if tag == "svg" {
                    &widget.template
                } else {
                    children
                        .iter()
                        .find(|c| matches!(c, crate::omni::types::HtmlNode::Element { .. }))
                        .expect("template should have an element child")
                }
            }
            _ => panic!(),
        };
        match svg {
            crate::omni::types::HtmlNode::Element {
                tag,
                classes,
                children,
                ..
            } => {
                assert_eq!(tag, "svg");
                assert!(classes.iter().any(|c| c == "omni-chart-bar"));
                // Should have track + fill rects
                assert_eq!(children.len(), 2);
                let fill_rect = &children[1];
                match fill_rect {
                    crate::omni::types::HtmlNode::Element {
                        tag, attributes, ..
                    } => {
                        assert_eq!(tag, "rect");
                        let height_attr = attributes.iter().find(|(k, _)| k == "height").unwrap();
                        assert!(height_attr.1.contains("bar_height"));
                        assert!(height_attr.1.contains("cpu.usage"));
                    }
                    _ => panic!(),
                }
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parse_chart_pie_desugars_to_svg_circles() {
        let source = r#"<widget id="mem" name="Memory">
<template>
  <chart type="pie" value="ram.used" total="ram.total"/>
</template>
<style></style>
</widget>"#;
        let (file, _diag) = parse_omni_with_diagnostics_hwinfo(source, false);
        let file = file.expect("parse succeeded");
        let widget = &file.widgets[0];
        // Adapt template navigation based on how your parser roots the template.
        // The svg element may be &widget.template directly (single-child case)
        // or children[0] of a wrapping div (multi-child case).
        let svg = match &widget.template {
            crate::omni::types::HtmlNode::Element { tag, children, .. } if tag == "svg" => {
                &widget.template
            }
            crate::omni::types::HtmlNode::Element { children, .. } => children
                .iter()
                .find(|c| matches!(c, crate::omni::types::HtmlNode::Element { tag, .. } if tag == "svg"))
                .expect("template should contain an svg element"),
            _ => panic!(),
        };
        match svg {
            crate::omni::types::HtmlNode::Element {
                tag,
                classes,
                children,
                ..
            } => {
                assert_eq!(tag, "svg");
                assert!(
                    classes.iter().any(|c| c == "omni-chart-pie"),
                    "svg should have omni-chart-pie class, got {:?}",
                    classes
                );
                assert_eq!(children.len(), 2, "pie should have track + fill circles");
                let fill_circle = &children[1];
                match fill_circle {
                    crate::omni::types::HtmlNode::Element {
                        tag, attributes, ..
                    } => {
                        assert_eq!(tag, "circle");
                        let dashoffset = attributes
                            .iter()
                            .find(|(k, _)| k == "stroke-dashoffset")
                            .expect("fill circle should have stroke-dashoffset");
                        assert!(
                            dashoffset.1.contains("ratio_dashoffset"),
                            "stroke-dashoffset should use ratio_dashoffset: {}",
                            dashoffset.1
                        );
                        assert!(dashoffset.1.contains("ram.used"));
                        assert!(dashoffset.1.contains("ram.total"));
                    }
                    _ => panic!("fill child should be Element"),
                }
            }
            _ => panic!("expected svg element"),
        }
    }

    #[test]
    fn collect_chart_sensors_finds_all_references() {
        let source = r#"<widget id="w1" name="W1">
<template>
  <chart type="line" sensor="cpu.usage"/>
  <chart type="bar" sensor="gpu.usage" min="0" max="100"/>
  <chart type="pie" value="ram.used" total="ram.total"/>
</template>
<style></style>
</widget>"#;
        let (file, _diag) = parse_omni_with_diagnostics_hwinfo(source, false);
        let file = file.expect("parse succeeded");
        let sensors = crate::omni::parser::collect_chart_sensors(&file);
        assert!(
            sensors.contains("cpu.usage"),
            "missing cpu.usage, got {:?}",
            sensors
        );
        assert!(sensors.contains("gpu.usage"), "missing gpu.usage");
        assert!(sensors.contains("ram.used"), "missing ram.used");
        assert!(sensors.contains("ram.total"), "missing ram.total");
    }

    #[test]
    fn parse_chart_card_desugars_to_full_layout() {
        let source = r#"<widget id="net" name="Network">
<template>
  <chart-card type="line" sensor="network.bytes_per_sec" unit="bytes/s" title="Network Down"/>
</template>
<style></style>
</widget>"#;
        let (file, _diag) = parse_omni_with_diagnostics_hwinfo(source, false);
        let file = file.expect("parse succeeded");
        let widget = &file.widgets[0];
        let svg = match &widget.template {
            crate::omni::types::HtmlNode::Element { tag, .. } if tag == "svg" => {
                &widget.template
            }
            crate::omni::types::HtmlNode::Element { children, .. } => children
                .iter()
                .find(|c| matches!(c, crate::omni::types::HtmlNode::Element { tag, .. } if tag == "svg"))
                .expect("template should contain an svg element"),
            _ => panic!(),
        };
        match svg {
            crate::omni::types::HtmlNode::Element {
                tag,
                classes,
                children,
                ..
            } => {
                assert_eq!(tag, "svg");
                assert!(
                    classes.iter().any(|c| c == "omni-chart-card"),
                    "svg should have omni-chart-card class, got {:?}",
                    classes
                );

                // Should contain a title text
                let title_found = children.iter().any(|c| {
                    matches!(c,
                        crate::omni::types::HtmlNode::Element { tag, classes, .. }
                        if tag == "text" && classes.iter().any(|cl| cl == "omni-chart-card-title")
                    )
                });
                assert!(title_found, "title text missing");

                // Should contain at least 2 Y-axis labels
                let y_label_count = children.iter().filter(|c| matches!(c,
                    crate::omni::types::HtmlNode::Element { tag, classes, .. }
                    if tag == "text" && classes.iter().any(|cl| cl == "omni-chart-card-y-label")
                )).count();
                assert!(
                    y_label_count >= 2,
                    "should have at least 2 y-labels, got {}",
                    y_label_count
                );

                // Should contain a <g class="omni-chart-card-plot" transform=...>
                // wrapping the inner chart shapes directly (no nested <svg>).
                let plot_group = children.iter().find_map(|node| {
                    if let crate::omni::types::HtmlNode::Element {
                        tag,
                        classes,
                        attributes,
                        children: inner_children,
                        ..
                    } = node
                    {
                        if tag == "g" && classes.iter().any(|c| c == "omni-chart-card-plot") {
                            return Some((attributes, inner_children));
                        }
                    }
                    None
                });
                let (attrs, shapes) = plot_group.expect("plot <g> wrapper missing");
                assert!(
                    attrs
                        .iter()
                        .any(|(k, v)| k == "transform" && v.starts_with("translate(")),
                    "plot group should have translate transform",
                );
                // The shapes inside should include a polyline for a line chart.
                assert!(
                    shapes.iter().any(|n| matches!(n,
                        crate::omni::types::HtmlNode::Element { tag, .. } if tag == "polyline"
                    )),
                    "plot group should contain a polyline",
                );
            }
            _ => panic!("expected svg element"),
        }
    }

    #[test]
    fn chart_card_deeply_nested_still_desugars() {
        // Reproduce a user-like scenario where <chart-card> sits several
        // levels deep inside the template, both self-closing and with
        // explicit end tag. No <chart-card> element should remain in the
        // resulting tree — desugar runs at every nesting level.
        let source = r#"<widget id="wrap" name="Wrap">
<template>
  <div class="outer">
    <div class="inner">
      <chart-card type="line" sensor="hwinfo.network.current_dl_rate" unit="bytes/s" title="Net"/>
      <chart-card type="bar" sensor="cpu.usage" min="0" max="100" title="CPU"></chart-card>
    </div>
  </div>
</template>
<style></style>
</widget>"#;
        let (file, diagnostics) = parse_omni_with_diagnostics_hwinfo(source, false);
        let file = file.expect("parse succeeded");

        // Assert no `chart-card` tag anywhere in the tree.
        fn walk_assert(node: &crate::omni::types::HtmlNode) {
            if let crate::omni::types::HtmlNode::Element { tag, children, .. } = node {
                assert_ne!(tag, "chart-card", "raw <chart-card> leaked into tree");
                assert_ne!(tag, "chart", "raw <chart> leaked into tree");
                for c in children {
                    walk_assert(c);
                }
            }
        }
        for w in &file.widgets {
            walk_assert(&w.template);
        }

        // Assert no "unknown element" diagnostics for chart-card/chart.
        for d in &diagnostics {
            assert!(
                !d.message.contains("<chart-card>") && !d.message.contains("<chart>"),
                "unexpected diagnostic mentioning raw chart tag: {}",
                d.message
            );
        }
    }

    #[test]
    fn parse_dpi_scale_auto() {
        let src = r#"
<config><dpi-scale value="auto"/></config>
<widget id="w" name="W" enabled="true">
<template><div>x</div></template><style></style>
</widget>
"#;
        let file = parse_omni(src).expect("parse");
        assert_eq!(file.dpi_scale, Some(DpiScale::Auto));
    }

    #[test]
    fn parse_dpi_scale_manual_15() {
        let src = r#"
<config><dpi-scale value="1.5"/></config>
<widget id="w" name="W" enabled="true">
<template><div>x</div></template><style></style>
</widget>
"#;
        let file = parse_omni(src).expect("parse");
        assert_eq!(file.dpi_scale, Some(DpiScale::Manual(1.5)));
    }

    #[test]
    fn parse_dpi_scale_zero_rejected() {
        let src = r#"
<config><dpi-scale value="0"/></config>
"#;
        let errs = parse_omni(src).expect_err("expected parse error");
        assert!(
            errs.iter().any(|e| e.message.contains("out of range")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn parse_dpi_scale_above_four_rejected() {
        let src = r#"
<config><dpi-scale value="5.0"/></config>
"#;
        let errs = parse_omni(src).expect_err("expected parse error");
        assert!(
            errs.iter().any(|e| e.message.contains("out of range")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn parse_dpi_scale_non_numeric_rejected() {
        let src = r#"
<config><dpi-scale value="banana"/></config>
"#;
        let errs = parse_omni(src).expect_err("expected parse error");
        assert!(
            errs.iter().any(|e| e.message.contains("not a number")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn parse_dpi_scale_missing_value_attr_rejected() {
        let src = r#"
<config><dpi-scale/></config>
"#;
        let errs = parse_omni(src).expect_err("expected parse error");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("requires a 'value'")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn parse_dpi_scale_duplicate_rejected() {
        let src = r#"
<config>
<dpi-scale value="auto"/>
<dpi-scale value="2.0"/>
</config>
"#;
        let errs = parse_omni(src).expect_err("expected parse error");
        assert!(
            errs.iter().any(|e| e.message.contains("duplicate")),
            "got: {:?}",
            errs
        );
    }

    #[test]
    fn parse_dpi_scale_absent_yields_none() {
        let src = r#"
<config><poll sensor="fps" interval="100"/></config>
<widget id="w" name="W" enabled="true">
<template><div>x</div></template><style></style>
</widget>
"#;
        let file = parse_omni(src).expect("parse");
        assert_eq!(file.dpi_scale, None);
    }

    #[test]
    fn dpi_scale_serde_roundtrip() {
        use crate::omni::types::DpiScale;
        for variant in [DpiScale::Auto, DpiScale::Manual(1.0), DpiScale::Manual(2.5)] {
            let json = serde_json::to_string(&variant).expect("ser");
            let back: DpiScale = serde_json::from_str(&json).expect("de");
            assert_eq!(back, variant, "round-trip mismatch for {:?}", variant);
        }
    }
}
