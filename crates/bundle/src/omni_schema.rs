//! Shared `.omni` schema constants consumed by both the host parser
//! (`crates/host/src/omni/parser.rs`) and the sanitizer
//! (`crates/sanitize/src/handlers/overlay.rs`).
//!
//! This module is the single source of truth for which element names,
//! attribute prefixes, and tag-specific attribute sets are legal in a
//! `.omni` file. Adding a new top-level element or custom tag starts
//! here — both validators pick it up on rebuild.
//!
//! Cross-reference: `feedback_spec_cross_reference_existing_parsers.md`
//! encodes the rule that new sanitize/validate specs must cite an
//! existing parser as source of truth. This module is that truth.
//!
//! WASM-clean: constants only, no std::fs, no threading, no IO.

/// Element names valid at the top level of a `.omni` file.
/// The file is a multi-root XML fragment; ALL of these may appear
/// zero or more times at depth 1.
pub const TOP_LEVEL_ELEMENTS: &[&str] = &["theme", "config", "widget"];

/// Element names valid as direct children of `<config>`.
/// - `<poll sensor="..." interval="..."/>` — per-sensor poll-interval override.
/// - `<dpi-scale value="auto|<float>"/>` — per-overlay device scale opt-in
///   (spec: `2026-04-25-overlay-dpi-scale-design.md`). Numeric `value` is
///   bounds-checked by the host parser at load time; the sanitizer here
///   only validates the element name to keep this module dependency-free
///   (matches the existing `<poll>` pattern — `sensor`/`interval` content
///   is similarly host-parser-validated).
pub const CONFIG_CHILDREN: &[&str] = &["poll", "dpi-scale"];

/// Element names valid as direct children of `<widget>`.
/// `<template>` is the required HTML body; `<style>` is optional CSS.
pub const WIDGET_CHILDREN: &[&str] = &["template", "style"];

/// HTML + SVG + Omni custom elements permitted inside `<template>` bodies.
/// The host parser emits warnings for anything not in this list; the
/// sanitizer strips anything not in this list via ammonia.
///
/// Chart custom elements: ONLY `<chart>` and `<chart-card>` appear in
/// source `.omni` files. Their `type="line|bar|pie"` attribute selects
/// the desugar path inside `omni/parser.rs::desugar_chart*` — "line",
/// "bar", and "pie" are attribute VALUES, not element names. Do NOT add
/// `chart-bar`, `chart-pie`, or `chart-line` here — they are not real
/// source elements.
pub const KNOWN_TEMPLATE_TAGS: &[&str] = &[
    // HTML
    "div",
    "span",
    "i",
    "p",
    "strong",
    "em",
    "br",
    "img",
    "section",
    "article",
    "header",
    "footer",
    "ul",
    "ol",
    "li",
    // SVG subset used by the chart system + hand-authored SVG
    "svg",
    "g",
    "polyline",
    "path",
    "rect",
    "circle",
    "line",
    "text",
    "ellipse",
    "polygon",
    "defs",
    "linearGradient",
    "radialGradient",
    "stop",
    // Omni source-level custom elements (desugared at parse time)
    "chart",
    "chart-card",
];

/// Attribute-name prefixes that survive sanitization on any tag.
/// `class:foo="expr"` is Omni's conditional-class directive syntax.
/// `data-sensor*` carries sensor-bound attributes for the renderer.
pub const TEMPLATE_ATTR_PREFIXES: &[&str] = &["class:", "data-"];

/// Attribute names universally allowed on any tag in the template body.
pub const UNIVERSAL_ATTRS: &[&str] = &["class", "id", "style", "title"];

/// Attribute names allowed on SVG tags in addition to UNIVERSAL_ATTRS.
pub const SVG_ATTRS: &[&str] = &[
    "x",
    "y",
    "width",
    "height",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "x1",
    "y1",
    "x2",
    "y2",
    "d",
    "points",
    "fill",
    "stroke",
    "stroke-width",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-opacity",
    "fill-opacity",
    "opacity",
    "transform",
    "transform-origin",
    "viewBox",
    "preserveAspectRatio",
    "text-anchor",
    "dominant-baseline",
    "dy",
    "dx",
    "gradientUnits",
    "gradientTransform",
    "spreadMethod",
    "offset",
    "stop-color",
    "stop-opacity",
    "font-family",
    "font-size",
];

/// Attribute names allowed on `<chart*>` tags in addition to UNIVERSAL_ATTRS.
pub const CHART_ATTRS: &[&str] = &[
    "sensor",
    "type",
    "unit",
    "title",
    "min",
    "max",
    "window",
    "format",
    "precision",
];

/// Attributes on `<img>` in addition to UNIVERSAL_ATTRS.
pub const IMG_ATTRS: &[&str] = &["src", "alt", "width", "height"];

/// SVG tag names — subset of KNOWN_TEMPLATE_TAGS that should receive
/// SVG_ATTRS in the sanitizer's per-tag attribute map.
pub const SVG_TAGS: &[&str] = &[
    "svg",
    "g",
    "polyline",
    "path",
    "rect",
    "circle",
    "line",
    "text",
    "ellipse",
    "polygon",
    "defs",
    "linearGradient",
    "radialGradient",
    "stop",
];

/// Chart tag names — subset of KNOWN_TEMPLATE_TAGS that should receive
/// CHART_ATTRS in the sanitizer's per-tag attribute map.
pub const CHART_TAGS: &[&str] = &["chart", "chart-card"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_is_theme_config_widget() {
        assert!(TOP_LEVEL_ELEMENTS.contains(&"theme"));
        assert!(TOP_LEVEL_ELEMENTS.contains(&"config"));
        assert!(TOP_LEVEL_ELEMENTS.contains(&"widget"));
        assert_eq!(TOP_LEVEL_ELEMENTS.len(), 3);
    }

    #[test]
    fn config_children_allows_poll_and_dpi_scale() {
        assert!(CONFIG_CHILDREN.contains(&"poll"));
        assert!(CONFIG_CHILDREN.contains(&"dpi-scale"));
        assert_eq!(CONFIG_CHILDREN.len(), 2);
    }

    #[test]
    fn widget_children_is_template_and_style() {
        assert!(WIDGET_CHILDREN.contains(&"template"));
        assert!(WIDGET_CHILDREN.contains(&"style"));
        assert_eq!(WIDGET_CHILDREN.len(), 2);
    }

    #[test]
    fn chart_tags_matches_parser_interception() {
        // If this test fails because CHART_TAGS changed, the host parser's
        // `desugar_chart*` interception in `crates/host/src/omni/parser.rs`
        // must learn the new element on the same commit.
        assert_eq!(CHART_TAGS, &["chart", "chart-card"]);
    }

    #[test]
    fn svg_tags_subset_of_known_template_tags() {
        for t in SVG_TAGS {
            assert!(
                KNOWN_TEMPLATE_TAGS.contains(t),
                "SVG_TAGS has {t}, missing from KNOWN_TEMPLATE_TAGS"
            );
        }
    }

    #[test]
    fn chart_tags_subset_of_known_template_tags() {
        for t in CHART_TAGS {
            assert!(
                KNOWN_TEMPLATE_TAGS.contains(t),
                "CHART_TAGS has {t}, missing from KNOWN_TEMPLATE_TAGS"
            );
        }
    }

    #[test]
    fn template_attr_prefixes_are_class_and_data() {
        assert!(TEMPLATE_ATTR_PREFIXES.contains(&"class:"));
        assert!(TEMPLATE_ATTR_PREFIXES.contains(&"data-"));
    }
}
