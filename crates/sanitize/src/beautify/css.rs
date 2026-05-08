//! CSS beautifier. Inverse of `handlers::theme::sanitize_css` — same
//! lightningcss parser, opposite minify flag.

use lightningcss::printer::PrinterOptions;
use lightningcss::stylesheet::{ParserOptions, StyleSheet};

use crate::beautify::error::BeautifyError;

pub(crate) fn beautify_css(bytes: &[u8]) -> Result<Vec<u8>, BeautifyError> {
    let css = std::str::from_utf8(bytes).map_err(|e| BeautifyError::Css(e.to_string()))?;
    let sheet = StyleSheet::parse(css, ParserOptions::default())
        .map_err(|e| BeautifyError::Css(e.to_string()))?;
    let result = sheet
        .to_css(PrinterOptions {
            minify: false,
            ..Default::default()
        })
        .map_err(|e| BeautifyError::Css(e.to_string()))?;
    Ok(result.code.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_prints_minified_input() {
        let input = b"body{color:red;margin:0}";
        let out = beautify_css(input).expect("valid CSS must beautify");
        let s = std::str::from_utf8(&out).expect("output must be utf-8");
        assert!(s.contains('\n'), "expected multi-line output, got: {s:?}");
        assert!(s.contains("color"), "output must preserve content");
    }

    #[test]
    fn idempotent_on_already_formatted() {
        let input = b"body {\n  color: red;\n  margin: 0;\n}\n";
        let first = beautify_css(input).expect("first pass ok");
        let second = beautify_css(&first).expect("second pass ok");
        assert_eq!(first, second, "beautify must be idempotent");
    }

    #[test]
    fn invalid_returns_err() {
        let input = b"body{:::garbage";
        let err = beautify_css(input).expect_err("invalid CSS must error");
        match err {
            BeautifyError::Css(_) => {}
            other => panic!("expected Css variant, got {other:?}"),
        }
    }

    #[test]
    fn empty_input_is_ok() {
        let out = beautify_css(b"").expect("empty input ok");
        assert!(out.is_empty() || out == b"\n");
    }
}
