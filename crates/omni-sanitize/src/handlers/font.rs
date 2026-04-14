use crate::error::{FileKind, SanitizeError};
use crate::handlers::Handler;

pub(crate) struct FontHandler;

impl Handler for FontHandler {
    fn kind(&self) -> &'static str { "font" }
    fn default_dir(&self) -> &'static str { "fonts" }
    fn default_extensions(&self) -> &'static [&'static str] { &["ttf", "otf", "woff2"] }
    fn default_max_size(&self) -> u64 { 1_572_864 }
    fn file_kind(&self) -> FileKind { FileKind::Font }
    fn sanitize(&self, path: &str, _bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
        Err(SanitizeError::Handler {
            kind: self.kind(),
            path: path.into(),
            detail: "font handler not implemented yet".into(),
            source: None,
        })
    }
}
