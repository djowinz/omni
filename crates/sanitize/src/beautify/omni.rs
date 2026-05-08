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

pub(crate) fn beautify_omni(bytes: &[u8]) -> Result<Vec<u8>, BeautifyError> {
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
        let beautified_str = std::str::from_utf8(&beautified)
            .map_err(|e| BeautifyError::Omni(format!("body utf-8: {e}")))?;
        let parent_indent = detect_parent_indent(bytes, range.start);
        let wrapped = wrap_body(beautified_str, &parent_indent);
        out.splice(range.start..range.end, wrapped.into_bytes());
    }
    Ok(out)
}

/// Indent (leading whitespace) of the line containing the opening tag whose
/// body starts at `body_start`. Returns "" when the tag is on a line that
/// begins with non-whitespace content (e.g. minified single-line input) so
/// the wrapper still produces a readable result without misattributing
/// preceding content as indentation.
fn detect_parent_indent(bytes: &[u8], body_start: usize) -> String {
    // Walk back past attributes to the '<' that opens the tag.
    let mut i = body_start;
    while i > 0 && bytes[i - 1] != b'<' {
        i -= 1;
    }
    if i == 0 {
        return String::new();
    }
    let tag_start = i - 1;

    // Walk back to start-of-line.
    let mut j = tag_start;
    while j > 0 && bytes[j - 1] != b'\n' {
        j -= 1;
    }

    let candidate = &bytes[j..tag_start];
    if candidate.iter().all(|b| matches!(*b, b' ' | b'\t')) {
        String::from_utf8_lossy(candidate).into_owned()
    } else {
        String::new()
    }
}

/// Wrap beautified body content so it sits one indent level deeper than the
/// parent tag: leading newline + body-indent on every content line, trailing
/// newline + parent-indent so the closing tag lines up with the opening tag.
/// Empty body short-circuits to a single newline + parent indent (avoids
/// emitting a stray indented blank line for `<template></template>`).
fn wrap_body(body: &str, parent_indent: &str) -> String {
    let trimmed = body.trim_matches('\n');
    if trimmed.is_empty() {
        let mut out = String::with_capacity(parent_indent.len() + 1);
        out.push('\n');
        out.push_str(parent_indent);
        return out;
    }
    let body_indent = format!("{parent_indent}  ");
    let mut out = String::with_capacity(body.len() + body_indent.len() * 8);
    out.push('\n');
    for line in trimmed.lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&body_indent);
            out.push_str(line);
            out.push('\n');
        }
    }
    out.push_str(parent_indent);
    out
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

    #[test]
    fn body_starts_on_new_line_after_opening_tag() {
        // Bug regression: previously `<style>.panel{...}` left the CSS body
        // on the same line as `<style>`. Body must start with a newline.
        let out = beautify_omni(MINIFIED_OMNI).expect("ok");
        let s = std::str::from_utf8(&out).expect("utf8");
        let after_style_open = &s[s.find("<style>").unwrap() + "<style>".len()..];
        assert!(
            after_style_open.starts_with('\n'),
            "body must start on a new line after <style>: {after_style_open:?}"
        );
        let after_tpl_open = &s[s.find("<template>").unwrap() + "<template>".len()..];
        assert!(
            after_tpl_open.starts_with('\n'),
            "body must start on a new line after <template>: {after_tpl_open:?}"
        );
    }

    #[test]
    fn body_inherits_parent_tag_indent() {
        // <style> sits at 4-space indent (inside <widget> inside the file).
        // The body must be indented one level deeper (6 spaces) and the
        // closing </style> must line up with its opener.
        let input = b"<widget>\n    <template>\n        <div/>\n    </template>\n    <style>body{color:red;margin:0}</style>\n</widget>\n";
        let out = beautify_omni(input).expect("ok");
        let s = std::str::from_utf8(&out).expect("utf8");

        let style_open = s.find("<style>").unwrap();
        let style_close = s.find("</style>").unwrap();
        let body = &s[style_open + "<style>".len()..style_close];

        // Body opens with `\n` + 6-space indent (parent indent 4 + body bump 2).
        assert!(
            body.starts_with("\n      "),
            "body must start with newline+6sp indent: {body:?}"
        );
        // Each non-blank content line carries at least 6sp of leading whitespace.
        for line in body.lines().filter(|l| !l.is_empty()) {
            assert!(
                line.starts_with("      "),
                "every body line must inherit parent indent: {line:?}"
            );
        }
        // Closing </style> lines up with its opener (4sp).
        assert!(
            body.ends_with("\n    "),
            "body must end with newline+4sp before </style>: {body:?}"
        );
    }

    #[test]
    fn parent_indent_falls_back_when_tag_shares_line_with_content() {
        // For minified input where <style> sits on a line with non-whitespace
        // siblings, indent detection must NOT misattribute that prefix as
        // indentation. Body wraps with empty parent indent.
        let out = beautify_omni(MINIFIED_OMNI).expect("ok");
        let s = std::str::from_utf8(&out).expect("utf8");
        let style_close = s.find("</style>").unwrap();
        let before_close = &s[..style_close];
        // The byte right before </style> should be '\n' (no preceding indent).
        assert!(
            before_close.ends_with('\n'),
            "minified parent => </style> sits at column 0 (newline before it)"
        );
    }
}
