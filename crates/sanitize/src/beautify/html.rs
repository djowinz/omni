//! HTML beautifier for `<template>` bodies inside `.omni` overlays.
//! Uses `markup_fmt` (pure-Rust formatter, used by dprint).

use markup_fmt::{config::FormatOptions, format_text, Language};

use crate::beautify::error::BeautifyError;

pub fn beautify_html(bytes: &[u8]) -> Result<Vec<u8>, BeautifyError> {
    let html = std::str::from_utf8(bytes).map_err(|e| BeautifyError::Html(e.to_string()))?;
    if html.trim().is_empty() {
        return Ok(bytes.to_vec());
    }
    let options = FormatOptions::default();
    let formatted = format_text(html, Language::Html, &options, |code, _| {
        Ok::<_, std::convert::Infallible>(code.into())
    })
    .map_err(|e| BeautifyError::Html(format!("{e:?}")))?;
    Ok(formatted.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_prints_minified_input() {
        let input = b"<div><span>a</span><span>b</span></div>";
        let out = beautify_html(input).expect("valid HTML must beautify");
        let s = std::str::from_utf8(&out).expect("output must be utf-8");
        assert!(s.contains('\n'), "expected multi-line output, got: {s:?}");
        assert!(s.contains("span"), "output must preserve content");
    }

    #[test]
    fn empty_input_is_ok() {
        let out = beautify_html(b"").expect("empty input ok");
        assert!(out.is_empty() || std::str::from_utf8(&out).unwrap().trim().is_empty());
    }

    #[test]
    fn whitespace_only_input_is_ok() {
        let out = beautify_html(b"   \n  \n").expect("whitespace ok");
        let s = std::str::from_utf8(&out).expect("output must be utf-8");
        assert!(s.trim().is_empty(), "output must be whitespace-equivalent");
    }
}
