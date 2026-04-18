//! Overlay handler — .omni XML validation + HTML sanitization of <template> body.

use std::collections::{HashMap, HashSet};

use ammonia::Builder;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{FileKind, SanitizeError};
use crate::handlers::Handler;

const MAX_DEPTH: usize = 16;
const ALLOWED_ROOT: &[&[u8]] = &[b"overlay", b"template", b"style", b"script"];

const DATA_SENSOR_ATTRS: &[&str] = &[
    "class",
    "id",
    "style",
    "data-sensor",
    "data-sensor-format",
    "data-sensor-precision",
    "data-sensor-threshold-warn",
    "data-sensor-threshold-critical",
    "data-sensor-target",
];

pub(crate) struct OverlayHandler;

impl Handler for OverlayHandler {
    fn kind(&self) -> &'static str { "overlay" }
    fn default_dir(&self) -> &'static str { "" }
    fn default_extensions(&self) -> &'static [&'static str] { &["omni"] }
    fn default_max_size(&self) -> u64 { 131_072 }
    fn file_kind(&self) -> FileKind { FileKind::Overlay }

    fn sanitize(&self, path: &str, bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
        validate_xml_structure(self.kind(), path, bytes)?;
        sanitize_template_body(self.kind(), path, bytes)
    }
}

fn validate_xml_structure(kind: &'static str, path: &str, bytes: &[u8]) -> Result<(), SanitizeError> {
    let mut reader = Reader::from_reader(bytes);
    reader.trim_text(true);
    reader.expand_empty_elements(false);
    reader.check_comments(true);

    let mut depth: usize = 0;
    let mut seen_root = false;
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => {
                return Err(SanitizeError::Handler {
                    kind, path: path.into(),
                    detail: format!("xml parse: {e}"), source: None,
                });
            }
            Ok(Event::Eof) => break,
            Ok(Event::DocType(_)) => {
                return Err(SanitizeError::Handler {
                    kind, path: path.into(),
                    detail: "DOCTYPE disallowed".into(), source: None,
                });
            }
            Ok(Event::PI(_)) => {
                return Err(SanitizeError::Handler {
                    kind, path: path.into(),
                    detail: "processing instruction disallowed".into(), source: None,
                });
            }
            Ok(Event::CData(_)) => {
                return Err(SanitizeError::Handler {
                    kind, path: path.into(),
                    detail: "CDATA disallowed".into(), source: None,
                });
            }
            Ok(Event::Start(ref e)) => {
                if !seen_root {
                    seen_root = true;
                    if !ALLOWED_ROOT.contains(&e.name().as_ref()) {
                        return Err(SanitizeError::Handler {
                            kind, path: path.into(),
                            detail: format!("unexpected root: {}", String::from_utf8_lossy(e.name().as_ref())),
                            source: None,
                        });
                    }
                }
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(SanitizeError::Handler {
                        kind, path: path.into(),
                        detail: format!("depth > {MAX_DEPTH}"), source: None,
                    });
                }
            }
            Ok(Event::End(_)) => { depth = depth.saturating_sub(1); }
            Ok(Event::Empty(ref e)) => {
                if !seen_root {
                    seen_root = true;
                    if !ALLOWED_ROOT.contains(&e.name().as_ref()) {
                        return Err(SanitizeError::Handler {
                            kind, path: path.into(),
                            detail: format!("unexpected root: {}", String::from_utf8_lossy(e.name().as_ref())),
                            source: None,
                        });
                    }
                }
            }
            Ok(_) => {}
        }
        buf.clear();
    }
    Ok(())
}

fn sanitize_template_body(kind: &'static str, path: &str, bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
    let src = std::str::from_utf8(bytes).map_err(|e| SanitizeError::Handler {
        kind, path: path.into(), detail: format!("utf8: {e}"), source: Some(Box::new(e)),
    })?;
    let (start_idx, body_start) = match src.find("<template>") {
        Some(i) => (i, i + "<template>".len()),
        None => return Ok(bytes.to_vec()),
    };
    let body_end = match src[body_start..].find("</template>") {
        Some(j) => body_start + j,
        None => {
            return Err(SanitizeError::Handler {
                kind, path: path.into(),
                detail: "unterminated <template>".into(), source: None,
            });
        }
    };
    let body = &src[body_start..body_end];

    let tags: HashSet<&str> = [
        "div", "span", "p", "h1", "h2", "h3", "h4", "h5", "h6", "strong", "em", "ul", "ol",
        "li", "img", "br", "section", "article", "header", "footer", "nav", "main", "figure",
        "figcaption",
    ].into_iter().collect();

    let mut tag_attrs: HashMap<&str, HashSet<&str>> = HashMap::new();
    for t in &tags {
        if *t == "img" {
            tag_attrs.insert(*t, ["src", "alt", "width", "height", "class", "id", "style"].into_iter().collect());
        } else {
            tag_attrs.insert(*t, DATA_SENSOR_ATTRS.iter().copied().collect());
        }
    }

    let cleaned = Builder::default()
        .tags(tags)
        .tag_attributes(tag_attrs)
        .url_schemes(HashSet::new())
        .strip_comments(true)
        .clean(body)
        .to_string();

    let mut out = String::with_capacity(src.len());
    out.push_str(&src[..start_idx]);
    out.push_str("<template>");
    out.push_str(&cleaned);
    out.push_str(&src[body_end..]);
    Ok(out.into_bytes())
}
