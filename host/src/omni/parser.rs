//! Parser for .omni file format.
//!
//! A .omni file contains:
//! - Optional `<theme src="..."/>` directive
//! - One or more `<widget id="..." name="..." enabled="true/false">` blocks
//!   - Each widget contains `<template>...</template>` and `<style>...</style>`

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::types::{HtmlNode, OmniFile, Widget};

/// A parse error with position information.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
}

/// Parse a .omni source string into an OmniFile.
pub fn parse_omni(source: &str) -> Result<OmniFile, Vec<ParseError>> {
    let mut errors = Vec::new();
    let mut theme_src = None;
    let mut widgets = Vec::new();

    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(true);

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => match e.name().as_ref() {
                b"widget" => match parse_widget(&mut reader, e) {
                    Ok(widget) => widgets.push(widget),
                    Err(e) => errors.push(e),
                },
                b"theme" => {
                    theme_src = get_attr(e, "src");
                }
                other => {
                    let name = String::from_utf8_lossy(other).to_string();
                    errors.push(ParseError {
                        message: format!("Unknown top-level element <{}>", name),
                        line: reader.buffer_position() as usize,
                    });
                }
            },
            Ok(Event::Empty(ref e)) => {
                if e.name().as_ref() == b"theme" {
                    theme_src = get_attr(e, "src");
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                errors.push(ParseError {
                    message: format!("XML parse error: {}", e),
                    line: reader.buffer_position() as usize,
                });
                break;
            }
            _ => {}
        }
    }

    if errors.is_empty() {
        Ok(OmniFile { theme_src, widgets })
    } else {
        Err(errors)
    }
}

/// Parse a `<widget>` element and its children (`<template>` and `<style>`).
fn parse_widget(reader: &mut Reader<&[u8]>, start: &BytesStart) -> Result<Widget, ParseError> {
    let id = get_attr(start, "id").ok_or_else(|| ParseError {
        message: "Widget missing required 'id' attribute".to_string(),
        line: reader.buffer_position() as usize,
    })?;

    let name = get_attr(start, "name").ok_or_else(|| ParseError {
        message: format!("Widget '{}' missing required 'name' attribute", id),
        line: reader.buffer_position() as usize,
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
                    template = Some(parse_template_children(reader)?);
                }
                b"style" => {
                    style_source = read_text_content(reader, "style")?;
                }
                _ => {}
            },
            Ok(Event::End(ref e)) if e.name().as_ref() == b"widget" => break,
            Ok(Event::Eof) => {
                return Err(ParseError {
                    message: format!("Unexpected EOF inside widget '{}'", id),
                    line: reader.buffer_position() as usize,
                });
            }
            Err(e) => {
                return Err(ParseError {
                    message: format!("XML error in widget '{}': {}", id, e),
                    line: reader.buffer_position() as usize,
                });
            }
            _ => {}
        }
    }

    let template = template.unwrap_or(HtmlNode::Element {
        tag: "div".to_string(),
        id: None,
        classes: vec![],
        inline_style: None,
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
fn parse_template_children(reader: &mut Reader<&[u8]>) -> Result<HtmlNode, ParseError> {
    let mut children = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let node = parse_html_element(reader, e)?;
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
                return Err(ParseError {
                    message: "Unexpected EOF inside <template>".to_string(),
                    line: reader.buffer_position() as usize,
                });
            }
            Err(e) => {
                return Err(ParseError {
                    message: format!("XML error in template: {}", e),
                    line: reader.buffer_position() as usize,
                });
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
            children,
        })
    }
}

/// Parse an HTML element with children.
fn parse_html_element(
    reader: &mut Reader<&[u8]>,
    start: &BytesStart,
) -> Result<HtmlNode, ParseError> {
    let tag = String::from_utf8_lossy(start.name().as_ref()).to_string();
    let id = get_attr(start, "id");
    let classes = get_attr(start, "class")
        .map(|c| c.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let inline_style = get_attr(start, "style");

    let mut children = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(ref e)) => {
                let child = parse_html_element(reader, e)?;
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
                return Err(ParseError {
                    message: format!("Unexpected EOF inside <{}>", tag),
                    line: reader.buffer_position() as usize,
                });
            }
            Err(e) => {
                return Err(ParseError {
                    message: format!("XML error in <{}>: {}", tag, e),
                    line: reader.buffer_position() as usize,
                });
            }
            _ => {}
        }
    }

    Ok(HtmlNode::Element {
        tag,
        id,
        classes,
        inline_style,
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

    HtmlNode::Element {
        tag,
        id,
        classes,
        inline_style,
        children: vec![],
    }
}

/// Read all text content until the closing tag is found.
fn read_text_content(reader: &mut Reader<&[u8]>, tag: &str) -> Result<String, ParseError> {
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
                return Err(ParseError {
                    message: format!("Unexpected EOF inside <{}>", tag),
                    line: reader.buffer_position() as usize,
                });
            }
            Err(e) => {
                return Err(ParseError {
                    message: format!("XML error reading <{}>: {}", tag, e),
                    line: reader.buffer_position() as usize,
                });
            }
            _ => {}
        }
    }

    Ok(content)
}

/// Extract an attribute value from an XML element.
fn get_attr(start: &BytesStart, name: &str) -> Option<String> {
    start
        .attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == name.as_bytes())
        .map(|a| String::from_utf8_lossy(&a.value).to_string())
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
    fn missing_widget_id_returns_error() {
        let source = r#"
            <widget name="Test" enabled="true">
                <template><div></div></template>
                <style></style>
            </widget>
        "#;

        let result = parse_omni(source);
        assert!(result.is_err());
        assert!(result.unwrap_err()[0]
            .message
            .contains("missing required 'id'"));
    }
}
