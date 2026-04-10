//! Parser for .omni file format.
//!
//! A .omni file contains:
//! - Optional `<theme src="..."/>` directive
//! - One or more `<widget id="..." name="..." enabled="true/false">` blocks
//!   - Each widget contains `<template>...</template>` and `<style>...</style>`

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use ts_rs::TS;

use super::types::{ConditionalClass, HtmlNode, OmniFile, Widget};
use super::validation;

/// Severity level for parse diagnostics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, TS)]
#[ts(export, export_to = "../../../apps/desktop/renderer/generated/")]
pub enum Severity {
    Error,
    Warning,
}

/// A parse error/warning with position and optional suggestion.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, TS)]
#[ts(export, export_to = "../../../apps/desktop/renderer/generated/")]
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
                    Ok(config) => poll_config = config,
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
                let node = parse_empty_html_element(e);
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
                children.push(parse_empty_html_element(e));
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
fn parse_empty_html_element(start: &BytesStart) -> HtmlNode {
    let tag = String::from_utf8_lossy(start.name().as_ref()).to_string();
    let id = get_attr(start, "id");
    let classes = get_attr(start, "class")
        .map(|c| c.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let inline_style = get_attr(start, "style");
    let conditional_classes = get_conditional_classes(start);
    let attributes = get_extra_attributes(start);

    HtmlNode::Element {
        tag,
        id,
        classes,
        inline_style,
        conditional_classes,
        attributes,
        children: vec![],
    }
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

/// Parse a `<config>` block containing `<poll sensor="..." interval="..." />` entries.
fn parse_config_block(
    source: &str,
    reader: &mut Reader<&[u8]>,
) -> Result<HashMap<String, u64>, ParseError> {
    let mut config = HashMap::new();

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"poll" => {
                let sensor = get_attr(e, "sensor");
                let interval = get_attr(e, "interval").and_then(|v| v.parse::<u64>().ok());

                if let (Some(sensor), Some(interval)) = (sensor, interval) {
                    config.insert(sensor, interval);
                }
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

    Ok(config)
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
            if key == b"id"
                || key == b"class"
                || key == b"style"
                || key.starts_with(class_prefix)
            {
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
}
