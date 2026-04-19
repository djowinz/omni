//! Overlay handler — matches the real `.omni` multi-root format.
//!
//! Format (see `docs/superpowers/specs/2026-04-18-omni-sanitizer-realignment-design.md`):
//!
//! - Multi-root XML fragment (no outer wrapper element)
//! - Top-level elements: `<theme src=".."/>`, `<config>...</config>`, `<widget ...>...</widget>`
//! - Inside `<config>`: `<poll sensor="..." interval="..."/>` empty elements only
//! - Inside `<widget>`: `<template>` (HTML body) and `<style>` (CSS body), both optional
//!
//! Validation strategy:
//!
//! 1. Single `quick_xml` event pass over the raw bytes.
//! 2. Enforce envelope depth <= 3 (legitimate files are <= 2 deep; 3 = sanity cap).
//! 3. At depth 1, assert element name is in `TOP_LEVEL_ELEMENTS`.
//! 4. Inside `<config>`, assert children in `CONFIG_CHILDREN` and are empty.
//! 5. Inside `<widget>`, assert children in `WIDGET_CHILDREN`.
//! 6. On `<template>` / `<style>` open at depth 2, switch to skip-mode:
//!    consume events until the matching close tag without validating tag
//!    names inside. Record the body's byte range for later splicing.
//! 7. Reject DOCTYPE, processing instructions, and CDATA at any envelope level.
//! 8. Validate `<theme src="...">` must be a relative workspace path.
//!
//! After validation, for each recorded body range:
//!
//! - `<template>` bodies are sanitized via ammonia with an allowlist sourced
//!   from `bundle::omni_schema` (tags, attribute prefixes, per-tag attrs).
//! - `<style>` bodies are sanitized via the reused `theme::sanitize_css`
//!   helper (lightningcss + URL whitelist + `@import` ban).
//!
//! Bytes outside recorded body ranges are copied through unchanged, preserving
//! author formatting (whitespace, widget attribute order, etc.).

use std::collections::{HashMap, HashSet};

use ammonia::Builder;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use bundle::omni_schema;

use crate::error::{FileKind, SanitizeError};
use crate::handlers::{theme, Handler};

const MAX_ENVELOPE_DEPTH: usize = 3;

pub(crate) struct OverlayHandler;

impl Handler for OverlayHandler {
    fn kind(&self) -> &'static str {
        "overlay"
    }
    fn default_dir(&self) -> &'static str {
        ""
    }
    fn default_extensions(&self) -> &'static [&'static str] {
        &["omni"]
    }
    fn default_max_size(&self) -> u64 {
        131_072
    }
    fn file_kind(&self) -> FileKind {
        FileKind::Overlay
    }

    fn sanitize(&self, path: &str, bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
        let ranges = validate_structure(self.kind(), path, bytes)?;
        let mut out = bytes.to_vec();
        // Splice in reverse so earlier byte offsets stay valid after later replacements.
        for range in ranges.into_iter().rev() {
            let body = &bytes[range.start..range.end];
            let sanitized = match range.kind {
                BodyKind::Template => sanitize_template_html(self.kind(), path, body)?,
                BodyKind::Style => theme::sanitize_css(self.kind(), path, body)?,
            };
            out.splice(range.start..range.end, sanitized.into_iter());
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, Copy)]
enum BodyKind {
    Template,
    Style,
}

#[derive(Debug, Clone)]
struct BodyRange {
    kind: BodyKind,
    start: usize,
    end: usize,
}

/// Structural validator. Single pass over `quick_xml` events, records body
/// ranges for later sanitization, returns them in order of appearance.
fn validate_structure(
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

/// Run ammonia over a `<template>` body using the allowlist sourced from
/// `bundle::omni_schema`.
fn sanitize_template_html(
    kind: &'static str,
    path: &str,
    body: &[u8],
) -> Result<Vec<u8>, SanitizeError> {
    let body_str = std::str::from_utf8(body).map_err(|e| SanitizeError::Handler {
        kind,
        path: path.into(),
        detail: format!("template utf8: {e}"),
        source: Some(Box::new(e)),
    })?;

    let tags: HashSet<&str> = omni_schema::KNOWN_TEMPLATE_TAGS.iter().copied().collect();
    let generic_attrs: HashSet<&str> = omni_schema::UNIVERSAL_ATTRS.iter().copied().collect();
    let attr_prefixes: HashSet<&str> = omni_schema::TEMPLATE_ATTR_PREFIXES
        .iter()
        .copied()
        .collect();

    let mut tag_attrs: HashMap<&str, HashSet<&str>> = HashMap::new();
    tag_attrs.insert("img", omni_schema::IMG_ATTRS.iter().copied().collect());
    let svg_attrs: HashSet<&str> = omni_schema::SVG_ATTRS.iter().copied().collect();
    for svg_tag in omni_schema::SVG_TAGS {
        tag_attrs.insert(svg_tag, svg_attrs.clone());
    }
    let chart_attrs: HashSet<&str> = omni_schema::CHART_ATTRS.iter().copied().collect();
    for chart_tag in omni_schema::CHART_TAGS {
        tag_attrs.insert(chart_tag, chart_attrs.clone());
    }

    let cleaned = Builder::default()
        .tags(tags)
        .generic_attributes(generic_attrs)
        .generic_attribute_prefixes(attr_prefixes)
        .tag_attributes(tag_attrs)
        .url_schemes(HashSet::new())
        .strip_comments(true)
        .clean(body_str)
        .to_string();

    Ok(cleaned.into_bytes())
}
