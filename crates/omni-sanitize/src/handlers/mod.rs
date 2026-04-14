//! Handler registry (retro-005 D5 / invariant #5). Adding a new built-in kind
//! = one new file in this module + one entry in HANDLERS. No existing handler
//! file changes.

use crate::error::{FileKind, SanitizeError};

pub(crate) mod font;
pub(crate) mod image;
pub(crate) mod overlay;
pub(crate) mod theme;

use font::FontHandler;
use image::ImageHandler;
use overlay::OverlayHandler;
use theme::ThemeHandler;

pub(crate) trait Handler: Sync {
    fn kind(&self) -> &'static str;
    fn default_dir(&self) -> &'static str;
    fn default_extensions(&self) -> &'static [&'static str];
    fn default_max_size(&self) -> u64;
    fn file_kind(&self) -> FileKind;
    fn sanitize(&self, path: &str, bytes: &[u8]) -> Result<Vec<u8>, SanitizeError>;
}

pub(crate) static HANDLERS: &[&(dyn Handler + Sync)] = &[
    &ThemeHandler,
    &FontHandler,
    &ImageHandler,
    &OverlayHandler,
];

pub(crate) fn supported_kind_names() -> Vec<&'static str> {
    HANDLERS.iter().map(|h| h.kind()).collect()
}

pub(crate) fn dispatch_for_path<'a>(
    path: &str,
    declared: Option<&'a std::collections::BTreeMap<String, omni_bundle::ResourceKind>>,
) -> Result<(&'static dyn Handler, u64), SanitizeError> {
    if let Some(decls) = declared {
        for (kind_name, rk) in decls {
            if matches_rk(path, &rk.dir, &rk.extensions) {
                let handler = HANDLERS
                    .iter()
                    .copied()
                    .find(|h| h.kind() == kind_name.as_str())
                    .ok_or_else(|| SanitizeError::UnknownResourceKind {
                        kind: kind_name.clone(),
                        supported: supported_kind_names(),
                    })?;
                return Ok((handler, rk.max_size_bytes));
            }
        }
    }
    for h in HANDLERS {
        let exts: Vec<String> = h.default_extensions().iter().map(|s| s.to_string()).collect();
        if matches_rk(path, h.default_dir(), &exts) {
            return Ok((*h, h.default_max_size()));
        }
    }
    Err(SanitizeError::UnknownResourceKind {
        kind: "<unrecognized>".into(),
        supported: supported_kind_names(),
    })
}

fn matches_rk(path: &str, dir: &str, exts: &[String]) -> bool {
    let ext_ok = exts.iter().any(|e| path.ends_with(&format!(".{e}")));
    if !ext_ok {
        return false;
    }
    if dir.is_empty() {
        !path.contains('/')
    } else {
        path.starts_with(&format!("{dir}/")) && {
            let after = &path[dir.len() + 1..];
            !after.contains('/')
        }
    }
}
