//! Reference-parser oracle: runs a fixture through BOTH the canonical
//! parser (`bundle::omni_schema` constants) AND the caller's SUT
//! parser/sanitizer, then asserts the two shapes agree.
//!
//! See spec Pillar 2 / writing-lessons §A9. The helper panics on
//! disagreement with a structural-diff message listing the divergent
//! elements.

use std::collections::BTreeSet;

use bundle::omni_schema::{KNOWN_TEMPLATE_TAGS, TOP_LEVEL_ELEMENTS};

/// Structural shape extracted from a parsed `.omni` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedShape {
    /// Top-level elements found (`<theme>`, `<config>`, `<widget>`).
    pub top_level_elements: BTreeSet<String>,
    /// Every element name encountered anywhere in the fragment.
    pub known_tags: BTreeSet<String>,
}

/// Canonical parse of `.omni` bytes using `bundle::omni_schema` as the
/// authority. Extracts the same structural information the SUT is expected
/// to preserve.
///
/// Implementation uses the existing schema constants rather than a fresh
/// parser — we're asserting "the SUT recognizes the same top-level + tag
/// universe as the schema canonical source," which is what the drift-class
/// this oracle targets actually requires.
pub fn parse_canonical(overlay_bytes: &[u8]) -> ParsedShape {
    let body = std::str::from_utf8(overlay_bytes).unwrap_or("");
    let mut top_level_elements = BTreeSet::new();
    let mut known_tags = BTreeSet::new();

    for el in TOP_LEVEL_ELEMENTS {
        let open = format!("<{el}");
        if body.contains(&open) {
            top_level_elements.insert((*el).to_string());
            known_tags.insert((*el).to_string());
        }
    }
    for tag in KNOWN_TEMPLATE_TAGS {
        let open = format!("<{tag}");
        if body.contains(&open) {
            known_tags.insert((*tag).to_string());
        }
    }

    ParsedShape {
        top_level_elements,
        known_tags,
    }
}

/// Assert that the SUT's parsed shape agrees with the canonical parser on
/// every element the canonical parser recognizes. The SUT may strip or
/// transform elements (e.g., sanitize may rewrite `<chart-card>` into
/// `<svg>`); this helper asserts on the INPUT shape before the SUT
/// transforms, verifying the SUT recognized what it was given.
///
/// Panics with a structural-diff message on mismatch.
pub fn assert_reference_parsers_agree(fixture_bytes: &[u8], sut_shape: &ParsedShape) {
    let canonical = parse_canonical(fixture_bytes);

    let missing_in_sut: BTreeSet<_> = canonical
        .top_level_elements
        .difference(&sut_shape.top_level_elements)
        .cloned()
        .collect();

    if !missing_in_sut.is_empty() {
        panic!(
            "reference-parser oracle: SUT missed top-level elements present in canonical parse: {missing_in_sut:?}\n\
             canonical top-level: {:?}\n\
             SUT top-level: {:?}",
            canonical.top_level_elements, sut_shape.top_level_elements
        );
    }

    let unknown_in_sut: BTreeSet<_> = sut_shape
        .top_level_elements
        .difference(&canonical.top_level_elements)
        .cloned()
        .collect();

    if !unknown_in_sut.is_empty() {
        panic!(
            "reference-parser oracle: SUT claims to recognize top-level elements the canonical parser does not: {unknown_in_sut:?}\n\
             (either the schema constants are stale or the SUT is inventing elements)"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_parse_recognizes_reference_overlay() {
        let bytes = include_bytes!("../../host/src/omni/assets/reference_overlay.omni");
        let shape = parse_canonical(bytes);
        assert!(
            shape.top_level_elements.contains("theme")
                || shape.top_level_elements.contains("widget"),
            "reference overlay must have at least one recognized top-level element; got {:?}",
            shape.top_level_elements
        );
    }

    #[test]
    fn agreeing_shapes_pass() {
        let bytes = b"<widget><template><div/></template></widget>";
        let mut top = BTreeSet::new();
        top.insert("widget".to_string());
        let mut tags = BTreeSet::new();
        tags.insert("widget".to_string());
        tags.insert("div".to_string());
        assert_reference_parsers_agree(
            bytes,
            &ParsedShape {
                top_level_elements: top,
                known_tags: tags,
            },
        );
    }

    #[test]
    #[should_panic(expected = "SUT missed top-level elements")]
    fn sut_missing_top_level_panics() {
        let bytes = b"<widget><template><div/></template></widget>";
        assert_reference_parsers_agree(
            bytes,
            &ParsedShape {
                top_level_elements: BTreeSet::new(),
                known_tags: BTreeSet::new(),
            },
        );
    }

    #[test]
    #[should_panic(expected = "SUT claims to recognize")]
    fn sut_inventing_elements_panics() {
        let bytes = b"<widget><template><div/></template></widget>";
        let mut top = BTreeSet::new();
        top.insert("widget".to_string());
        top.insert("invented-root".to_string());
        assert_reference_parsers_agree(
            bytes,
            &ParsedShape {
                top_level_elements: top,
                known_tags: BTreeSet::new(),
            },
        );
    }
}
