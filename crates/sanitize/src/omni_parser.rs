//! `.omni` multi-root parser. Source of truth for both `handlers::overlay`
//! (security-side sanitize) and `beautify::omni` (fork-time pretty-print).
//!
//! The parser walks `quick_xml` events, validates envelope structure
//! (max depth 3, top-level / nested element name allowlists), and records
//! the byte ranges of `<template>` and `<style>` bodies inside `<widget>`
//! for later splicing.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use bundle::omni_schema;

use crate::error::SanitizeError;

pub(crate) const MAX_ENVELOPE_DEPTH: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyKind {
    Template,
    Style,
}

#[derive(Debug, Clone)]
pub(crate) struct BodyRange {
    pub(crate) kind: BodyKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Structural validator. Single pass over `quick_xml` events, records body
/// ranges for later sanitization, returns them in order of appearance.
pub(crate) fn validate_structure(
    kind: &'static str,
    path: &str,
    bytes: &[u8],
) -> Result<Vec<BodyRange>, SanitizeError> {
    let mut reader = Reader::from_reader(bytes);
    reader.trim_text(false);
    reader.expand_empty_elements(false);
    reader.check_comments(true);

    let mut ranges: Vec<BodyRange> = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => {
                return Err(handler_err(kind, path, format!("xml parse: {e}")));
            }
            Ok(Event::Eof) => break,
            Ok(Event::DocType(_)) => {
                return Err(handler_err(kind, path, "DOCTYPE disallowed".into()));
            }
            Ok(Event::PI(_)) => {
                return Err(handler_err(
                    kind,
                    path,
                    "processing instruction disallowed".into(),
                ));
            }
            Ok(Event::CData(_)) => {
                return Err(handler_err(kind, path, "CDATA disallowed".into()));
            }
            Ok(Event::Start(ref e)) => {
                let name = elem_name(e);
                let depth = stack.len() + 1;
                if depth > MAX_ENVELOPE_DEPTH {
                    return Err(handler_err(
                        kind,
                        path,
                        format!("envelope depth > {MAX_ENVELOPE_DEPTH}"),
                    ));
                }
                validate_envelope_start(kind, path, &name, depth, &stack, e)?;

                // Enter skip-mode for <template> / <style> at depth 2 inside <widget>.
                if depth == 2
                    && stack.last().map(|s| s.as_str()) == Some("widget")
                    && (name == "template" || name == "style")
                {
                    let body_start = reader.buffer_position();
                    let body_kind = if name == "template" {
                        BodyKind::Template
                    } else {
                        BodyKind::Style
                    };
                    let body_end = skip_to_close(&mut reader, &name, kind, path)?;
                    ranges.push(BodyRange {
                        kind: body_kind,
                        start: body_start,
                        end: body_end,
                    });
                    // skip_to_close consumed the matching </name>, do NOT push onto stack.
                    continue;
                }

                stack.push(name);
            }
            Ok(Event::End(_)) => {
                stack.pop();
            }
            Ok(Event::Empty(ref e)) => {
                let name = elem_name(e);
                let depth = stack.len() + 1;
                if depth > MAX_ENVELOPE_DEPTH {
                    return Err(handler_err(
                        kind,
                        path,
                        format!("envelope depth > {MAX_ENVELOPE_DEPTH}"),
                    ));
                }
                validate_envelope_empty(kind, path, &name, depth, &stack, e)?;
                // Empty elements do not push onto the stack.
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(ranges)
}

fn elem_name(e: &BytesStart) -> String {
    String::from_utf8_lossy(e.name().as_ref()).into_owned()
}

fn handler_err(kind: &'static str, path: &str, detail: String) -> SanitizeError {
    SanitizeError::Handler {
        kind,
        path: path.into(),
        detail,
        source: None,
    }
}

fn validate_envelope_start(
    kind: &'static str,
    path: &str,
    name: &str,
    depth: usize,
    stack: &[String],
    e: &BytesStart,
) -> Result<(), SanitizeError> {
    match depth {
        1 => {
            if !omni_schema::TOP_LEVEL_ELEMENTS.contains(&name) {
                return Err(handler_err(
                    kind,
                    path,
                    format!("unexpected top-level <{name}>"),
                ));
            }
            if name == "theme" {
                validate_theme_src(kind, path, e)?;
            }
            Ok(())
        }
        2 => {
            let parent = stack.last().map(String::as_str).unwrap_or("");
            match parent {
                "config" => {
                    if !omni_schema::CONFIG_CHILDREN.contains(&name) {
                        return Err(handler_err(
                            kind,
                            path,
                            format!("<config> child <{name}> not permitted"),
                        ));
                    }
                    Err(handler_err(
                        kind,
                        path,
                        format!("<poll> must be empty/self-closing, got <{name}> with content"),
                    ))
                }
                "widget" => {
                    if !omni_schema::WIDGET_CHILDREN.contains(&name) {
                        return Err(handler_err(
                            kind,
                            path,
                            format!("<widget> child <{name}> not permitted"),
                        ));
                    }
                    Ok(())
                }
                other => Err(handler_err(
                    kind,
                    path,
                    format!("<{name}> not permitted inside <{other}>"),
                )),
            }
        }
        _ => Err(handler_err(
            kind,
            path,
            format!("envelope depth {depth} > {MAX_ENVELOPE_DEPTH}"),
        )),
    }
}

fn validate_envelope_empty(
    kind: &'static str,
    path: &str,
    name: &str,
    depth: usize,
    stack: &[String],
    e: &BytesStart,
) -> Result<(), SanitizeError> {
    match depth {
        1 => {
            if !omni_schema::TOP_LEVEL_ELEMENTS.contains(&name) {
                return Err(handler_err(
                    kind,
                    path,
                    format!("unexpected top-level <{name}/>"),
                ));
            }
            if name == "theme" {
                validate_theme_src(kind, path, e)?;
            }
            Ok(())
        }
        2 => {
            let parent = stack.last().map(String::as_str).unwrap_or("");
            match parent {
                "config" => {
                    if !omni_schema::CONFIG_CHILDREN.contains(&name) {
                        return Err(handler_err(
                            kind,
                            path,
                            format!("<config> child <{name}/> not permitted"),
                        ));
                    }
                    Ok(())
                }
                "widget" => {
                    if !omni_schema::WIDGET_CHILDREN.contains(&name) {
                        return Err(handler_err(
                            kind,
                            path,
                            format!("<widget> child <{name}/> not permitted"),
                        ));
                    }
                    Ok(())
                }
                other => Err(handler_err(
                    kind,
                    path,
                    format!("<{name}/> not permitted inside <{other}>"),
                )),
            }
        }
        _ => Err(handler_err(
            kind,
            path,
            format!("envelope depth {depth} > {MAX_ENVELOPE_DEPTH}"),
        )),
    }
}

fn validate_theme_src(kind: &'static str, path: &str, e: &BytesStart) -> Result<(), SanitizeError> {
    let src = e
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"src")
        .and_then(|a| String::from_utf8(a.value.into_owned()).ok())
        .unwrap_or_default();
    if src.is_empty() {
        return Ok(());
    }
    if src.contains("://") {
        return Err(handler_err(
            kind,
            path,
            format!("<theme> src must be a relative workspace path (got {src:?})"),
        ));
    }
    if src.starts_with('/') || src.starts_with('\\') {
        return Err(handler_err(
            kind,
            path,
            format!("<theme> src must be relative, not absolute (got {src:?})"),
        ));
    }
    if src.split(['/', '\\']).any(|seg| seg == "..") {
        return Err(handler_err(
            kind,
            path,
            format!("<theme> src must not contain '..' (got {src:?})"),
        ));
    }
    Ok(())
}

/// Consume `quick_xml` events inside a skip-mode body until the matching
/// close tag for `tag`. Returns the byte position of the `<` of the close tag,
/// so `[body_start..body_end]` is exactly the body content between the open
/// tag's `>` and the close tag's `<`.
fn skip_to_close(
    reader: &mut Reader<&[u8]>,
    tag: &str,
    kind: &'static str,
    path: &str,
) -> Result<usize, SanitizeError> {
    let mut depth: usize = 1;
    let mut buf = Vec::new();
    let tag_bytes = tag.as_bytes();
    loop {
        let before = reader.buffer_position();
        match reader.read_event_into(&mut buf) {
            Err(e) => {
                return Err(handler_err(
                    kind,
                    path,
                    format!("xml parse inside <{tag}>: {e}"),
                ));
            }
            Ok(Event::Eof) => {
                return Err(handler_err(kind, path, format!("unterminated <{tag}>")));
            }
            Ok(Event::DocType(_)) | Ok(Event::PI(_)) | Ok(Event::CData(_)) => {
                // Body content may contain these inside strings/comments that the
                // wrapping handler (ammonia or theme CSS) will catch. Not our
                // structural concern.
            }
            Ok(Event::Start(ref e)) => {
                if e.name().as_ref() == tag_bytes {
                    depth += 1;
                }
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == tag_bytes {
                    depth -= 1;
                    if depth == 0 {
                        buf.clear();
                        return Ok(before);
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
}
