//! `.omni` beautifier. Uses `crate::omni_parser::validate_structure` to
//! locate `<template>` and `<style>` body byte ranges, beautifies each
//! body, and splices the results back into the original buffer in
//! reverse order so earlier offsets stay valid.
//!
//! Outer XML (everything outside body ranges) is preserved as-authored —
//! same invariant the sanitize handler maintains.

use crate::beautify::css::beautify_css;
use crate::beautify::error::BeautifyError;
use crate::beautify::html::beautify_html;
use crate::omni_parser::{validate_structure, BodyKind};

pub fn beautify_omni(bytes: &[u8]) -> Result<Vec<u8>, BeautifyError> {
    let ranges = validate_structure("beautify", "<fork>", bytes)
        .map_err(|e| BeautifyError::Omni(format!("parse: {e}")))?;

    let mut out = bytes.to_vec();
    // Splice in reverse so earlier offsets stay valid after later replacements.
    for range in ranges.into_iter().rev() {
        let body = &bytes[range.start..range.end];
        let beautified = match range.kind {
            BodyKind::Template => beautify_html(body)
                .map_err(|e| BeautifyError::Omni(format!("template: {e}")))?,
            BodyKind::Style => beautify_css(body)
                .map_err(|e| BeautifyError::Omni(format!("style: {e}")))?,
        };
        out.splice(range.start..range.end, beautified);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIFIED_OMNI: &[u8] = b"<widget><template><div><span>a</span></div></template><style>body{color:red;margin:0}</style></widget>";

    #[test]
    fn pretties_style_body() {
        let out = beautify_omni(MINIFIED_OMNI).expect("valid omni must beautify");
        let s = std::str::from_utf8(&out).expect("output utf-8");
        // Style body is now multi-line.
        let style_open = s.find("<style>").unwrap();
        let style_close = s.find("</style>").unwrap();
        let body = &s[style_open + "<style>".len()..style_close];
        assert!(body.contains('\n'), "<style> body must be multi-line: {body:?}");
        assert!(body.contains("color"), "<style> body must preserve content");
    }

    #[test]
    fn pretties_template_body() {
        let out = beautify_omni(MINIFIED_OMNI).expect("valid omni must beautify");
        let s = std::str::from_utf8(&out).expect("output utf-8");
        let tpl_open = s.find("<template>").unwrap();
        let tpl_close = s.find("</template>").unwrap();
        let body = &s[tpl_open + "<template>".len()..tpl_close];
        assert!(body.contains('\n'), "<template> body must be multi-line: {body:?}");
        assert!(body.contains("span"), "<template> body must preserve content");
    }

    #[test]
    fn preserves_outer_xml_outside_body_ranges() {
        // Walk the parser ourselves to know body ranges, then assert bytes
        // outside those ranges are byte-identical between input and output.
        let ranges = validate_structure("test", "fixture.omni", MINIFIED_OMNI)
            .expect("fixture must parse");
        let out = beautify_omni(MINIFIED_OMNI).expect("must beautify");
        // Build a "cut out the body ranges" projection of both sides.
        // After beautify, body offsets shift, so we splice by re-running
        // the parser on the OUTPUT and comparing the non-body bytes
        // segment-by-segment with the input's non-body bytes.
        let out_ranges = validate_structure("test", "fixture.omni", &out)
            .expect("output must parse");
        assert_eq!(ranges.len(), out_ranges.len(), "same body count");
        let mut prev_in_end = 0usize;
        let mut prev_out_end = 0usize;
        for (rin, rout) in ranges.iter().zip(out_ranges.iter()) {
            assert_eq!(rin.kind, rout.kind);
            // Bytes between prev end and this body's start must match.
            assert_eq!(
                &MINIFIED_OMNI[prev_in_end..rin.start],
                &out[prev_out_end..rout.start],
                "outer XML diverged before {:?} body",
                rin.kind
            );
            prev_in_end = rin.end;
            prev_out_end = rout.end;
        }
        // Trailing bytes after last body.
        assert_eq!(&MINIFIED_OMNI[prev_in_end..], &out[prev_out_end..]);
    }

    #[test]
    fn handles_empty_template() {
        let input = b"<widget><template></template><style>a{}</style></widget>";
        let out = beautify_omni(input).expect("empty template ok");
        // Output must still parse.
        validate_structure("test", "fixture.omni", &out).expect("output parses");
    }

    #[test]
    fn handles_no_widget() {
        let input = b"<theme src=\"themes/dark.css\"/>\n";
        let out = beautify_omni(input).expect("theme-only ok");
        assert_eq!(out, input, "no body ranges => byte-equal output");
    }

    #[test]
    fn invalid_inner_css_returns_err() {
        let input = b"<widget><style>body{:::garbage</style></widget>";
        let err = beautify_omni(input).expect_err("malformed CSS must error");
        match err {
            BeautifyError::Omni(_) => {}
            other => panic!("expected Omni variant, got {other:?}"),
        }
    }

    #[test]
    fn malformed_xml_returns_err() {
        let input = b"<widget><template>unclosed";
        let err = beautify_omni(input).expect_err("malformed XML must error");
        match err {
            BeautifyError::Omni(_) => {}
            other => panic!("expected Omni variant, got {other:?}"),
        }
    }
}
