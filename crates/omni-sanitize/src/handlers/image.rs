use crate::error::{FileKind, SanitizeError};
use crate::handlers::Handler;

pub(crate) struct ImageHandler;

impl Handler for ImageHandler {
    fn kind(&self) -> &'static str { "image" }
    fn default_dir(&self) -> &'static str { "images" }
    fn default_extensions(&self) -> &'static [&'static str] { &["png", "jpg", "jpeg", "webp"] }
    fn default_max_size(&self) -> u64 { 1_572_864 }
    fn file_kind(&self) -> FileKind { FileKind::Image }
    fn sanitize(&self, path: &str, _bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
        Err(SanitizeError::Handler {
            kind: self.kind(),
            path: path.into(),
            detail: "image handler not implemented yet".into(),
            source: None,
        })
    }
}
