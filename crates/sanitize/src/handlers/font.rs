//! Font handler — ttf-parser structural validation + magic gate.

use crate::error::{FileKind, SanitizeError};
use crate::handlers::Handler;

pub(crate) struct FontHandler;

impl Handler for FontHandler {
    fn kind(&self) -> &'static str {
        "font"
    }
    fn default_dir(&self) -> &'static str {
        "fonts"
    }
    fn default_extensions(&self) -> &'static [&'static str] {
        &["ttf", "otf", "woff2"]
    }
    fn default_max_size(&self) -> u64 {
        1_572_864
    }
    fn file_kind(&self) -> FileKind {
        FileKind::Font
    }

    fn sanitize(&self, path: &str, bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
        if bytes.len() < 4 {
            return Err(SanitizeError::Handler {
                kind: self.kind(),
                path: path.into(),
                detail: "too short".into(),
                source: None,
            });
        }
        let magic = &bytes[0..4];
        let magic_ok = matches!(
            magic,
            [0x00, 0x01, 0x00, 0x00] | b"OTTO" | b"wOF2" | b"true" | b"typ1"
        );
        if !magic_ok {
            return Err(SanitizeError::Handler {
                kind: self.kind(),
                path: path.into(),
                detail: format!("bad magic {magic:02x?}"),
                source: None,
            });
        }

        ttf_parser::Face::parse(bytes, 0).map_err(|e| SanitizeError::Handler {
            kind: self.kind(),
            path: path.into(),
            detail: format!("ttf-parser: {e}"),
            source: None,
        })?;

        Ok(bytes.to_vec())
    }
}
