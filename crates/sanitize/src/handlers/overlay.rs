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

use bundle::omni_schema;

use crate::error::{FileKind, SanitizeError};
use crate::handlers::{theme, Handler};
use crate::omni_parser::{validate_structure, BodyKind};

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
