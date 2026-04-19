//! Theme handler — CSS parse + URL whitelist + minified re-emit via lightningcss.

use lightningcss::printer::PrinterOptions;
use lightningcss::stylesheet::{ParserOptions, StyleSheet};

use crate::error::{FileKind, SanitizeError};
use crate::handlers::Handler;

pub(crate) struct ThemeHandler;

impl Handler for ThemeHandler {
    fn kind(&self) -> &'static str {
        "theme"
    }
    fn default_dir(&self) -> &'static str {
        "themes"
    }
    fn default_extensions(&self) -> &'static [&'static str] {
        &["css"]
    }
    fn default_max_size(&self) -> u64 {
        131_072
    }
    fn file_kind(&self) -> FileKind {
        FileKind::Theme
    }

    fn sanitize(&self, path: &str, bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
        sanitize_css(self.kind(), path, bytes)
    }
}

/// CSS sanitization pipeline. Exposed at crate visibility so the overlay
/// handler can reuse this for widget `<style>` body sanitization without
/// duplicating the lightningcss + URL-whitelist logic.
///
/// `kind` is passed through to error variants so callers can tag errors
/// with the originating handler name ("theme" for standalone themes,
/// "overlay" for embedded styles inside an overlay).
pub(crate) fn sanitize_css(
    kind: &'static str,
    path: &str,
    bytes: &[u8],
) -> Result<Vec<u8>, SanitizeError> {
    let src = std::str::from_utf8(bytes).map_err(|e| SanitizeError::Handler {
        kind,
        path: path.into(),
        detail: format!("utf8: {e}"),
        source: Some(Box::new(e)),
    })?;

    let lower = src.to_ascii_lowercase();
    if lower.contains("@import") {
        return Err(SanitizeError::Handler {
            kind,
            path: path.into(),
            detail: "@import disallowed".into(),
            source: None,
        });
    }
    scan_urls(kind, path, src, &lower)?;

    let sheet =
        StyleSheet::parse(src, ParserOptions::default()).map_err(|e| SanitizeError::Handler {
            kind,
            path: path.into(),
            detail: format!("parse: {e}"),
            source: None,
        })?;

    let printed = sheet
        .to_css(PrinterOptions {
            minify: true,
            ..Default::default()
        })
        .map_err(|e| SanitizeError::Handler {
            kind,
            path: path.into(),
            detail: format!("print: {e}"),
            source: None,
        })?;

    Ok(printed.code.into_bytes())
}

fn scan_urls(kind: &'static str, path: &str, src: &str, lower: &str) -> Result<(), SanitizeError> {
    let mut i = 0;
    while let Some(idx) = lower[i..].find("url(") {
        let start = i + idx + 4;
        let rest = &src[start..];
        let end = rest.find(')').ok_or_else(|| SanitizeError::Handler {
            kind,
            path: path.into(),
            detail: "unterminated url()".into(),
            source: None,
        })?;
        let arg = rest[..end].trim().trim_matches(|c| c == '\'' || c == '"');
        validate_url(kind, path, arg)?;
        i = start + end + 1;
    }
    Ok(())
}

fn validate_url(kind: &'static str, path: &str, u: &str) -> Result<(), SanitizeError> {
    let lower = u.to_ascii_lowercase();
    if lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("file://")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
    {
        return Err(SanitizeError::Handler {
            kind,
            path: path.into(),
            detail: format!("disallowed scheme in url(): {u}"),
            source: None,
        });
    }
    if u.starts_with('/') || u.contains("..") {
        return Err(SanitizeError::Handler {
            kind,
            path: path.into(),
            detail: format!("unsafe url(): {u}"),
            source: None,
        });
    }
    if u.split('/').count() > 2 {
        return Err(SanitizeError::Handler {
            kind,
            path: path.into(),
            detail: format!("url() too deep: {u}"),
            source: None,
        });
    }
    Ok(())
}
