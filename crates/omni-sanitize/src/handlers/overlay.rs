use crate::error::{FileKind, SanitizeError};
use crate::handlers::Handler;

pub(crate) struct OverlayHandler;

impl Handler for OverlayHandler {
    fn kind(&self) -> &'static str { "overlay" }
    fn default_dir(&self) -> &'static str { "" }
    fn default_extensions(&self) -> &'static [&'static str] { &["omni"] }
    fn default_max_size(&self) -> u64 { 131_072 }
    fn file_kind(&self) -> FileKind { FileKind::Overlay }
    fn sanitize(&self, path: &str, _bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
        Err(SanitizeError::Handler {
            kind: self.kind(),
            path: path.into(),
            detail: "overlay handler not implemented yet".into(),
            source: None,
        })
    }
}
