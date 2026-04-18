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
pub(crate) use theme::ThemeHandler;

pub(crate) trait Handler: Sync {
    fn kind(&self) -> &'static str;
    fn default_dir(&self) -> &'static str;
    fn default_extensions(&self) -> &'static [&'static str];
    fn default_max_size(&self) -> u64;
    fn file_kind(&self) -> FileKind;
    fn sanitize(&self, path: &str, bytes: &[u8]) -> Result<Vec<u8>, SanitizeError>;
}

pub(crate) static HANDLERS: &[&(dyn Handler + Sync)] =
    &[&ThemeHandler, &FontHandler, &ImageHandler, &OverlayHandler];

pub(crate) fn supported_kind_names() -> Vec<&'static str> {
    HANDLERS.iter().map(|h| h.kind()).collect()
}

pub(crate) fn dispatch_for_path(
    path: &str,
    declared: Option<&std::collections::BTreeMap<String, bundle::ResourceKind>>,
) -> Result<(&'static dyn Handler, u64), SanitizeError> {
    if let Some(decls) = declared {
        for (kind_name, rk) in decls {
            if matches_dir_ext(path, &rk.dir, rk.extensions.iter().map(String::as_str)) {
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
        if matches_dir_ext(
            path,
            h.default_dir(),
            h.default_extensions().iter().copied(),
        ) {
            return Ok((*h, h.default_max_size()));
        }
    }
    Err(SanitizeError::UnknownResourceKind {
        kind: "<unrecognized>".into(),
        supported: supported_kind_names(),
    })
}

fn matches_dir_ext<'a, I>(path: &str, dir: &str, exts: I) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    let ext_ok = exts.into_iter().any(|e| {
        path.len() > e.len() + 1
            && path.ends_with(e)
            && path.as_bytes()[path.len() - e.len() - 1] == b'.'
    });
    if !ext_ok {
        return false;
    }
    if dir.is_empty() {
        !path.contains('/')
    } else {
        let prefix_len = dir.len() + 1;
        path.len() > prefix_len
            && path.as_bytes()[dir.len()] == b'/'
            && path.starts_with(dir)
            && !path[prefix_len..].contains('/')
    }
}
