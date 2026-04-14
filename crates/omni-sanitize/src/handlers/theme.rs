use crate::error::{FileKind, SanitizeError};
use crate::handlers::Handler;

pub(crate) struct ThemeHandler;

impl Handler for ThemeHandler {
    fn kind(&self) -> &'static str { "theme" }
    fn default_dir(&self) -> &'static str { "themes" }
    fn default_extensions(&self) -> &'static [&'static str] { &["css"] }
    fn default_max_size(&self) -> u64 { 131_072 }
    fn file_kind(&self) -> FileKind { FileKind::Theme }
    fn sanitize(&self, path: &str, _bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
        Err(SanitizeError::Handler {
            kind: self.kind(),
            path: path.into(),
            detail: "theme handler not implemented yet".into(),
            source: None,
        })
    }
}
