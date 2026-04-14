//! Image handler — decode, optional downscale, re-encode as PNG.

use std::io::Cursor;

use image::{imageops::FilterType, GenericImageView, ImageFormat};

use crate::error::{FileKind, SanitizeError};
use crate::handlers::Handler;

const MAX_W: u32 = 1920;
const MAX_H: u32 = 1080;
const MAX_REENCODED: u64 = 1_048_576;

pub(crate) struct ImageHandler;

impl Handler for ImageHandler {
    fn kind(&self) -> &'static str { "image" }
    fn default_dir(&self) -> &'static str { "images" }
    fn default_extensions(&self) -> &'static [&'static str] { &["png", "jpg", "jpeg", "webp"] }
    fn default_max_size(&self) -> u64 { 1_572_864 }
    fn file_kind(&self) -> FileKind { FileKind::Image }

    fn sanitize(&self, path: &str, bytes: &[u8]) -> Result<Vec<u8>, SanitizeError> {
        let fmt = ImageFormat::from_path(path).map_err(|e| SanitizeError::Handler {
            kind: self.kind(),
            path: path.into(),
            detail: format!("bad ext: {e}"),
            source: None,
        })?;
        let img = image::load_from_memory_with_format(bytes, fmt).map_err(|e| {
            SanitizeError::Handler {
                kind: self.kind(),
                path: path.into(),
                detail: format!("decode: {e}"),
                source: None,
            }
        })?;
        let (w, h) = img.dimensions();
        let scaled = if w > MAX_W || h > MAX_H {
            img.resize(MAX_W, MAX_H, FilterType::Lanczos3)
        } else {
            img
        };
        let mut out = Vec::new();
        scaled
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .map_err(|e| SanitizeError::Handler {
                kind: self.kind(),
                path: path.into(),
                detail: format!("encode: {e}"),
                source: None,
            })?;
        if (out.len() as u64) > MAX_REENCODED {
            return Err(SanitizeError::SizeExceeded {
                path: path.into(),
                actual: out.len() as u64,
                limit: MAX_REENCODED,
            });
        }
        Ok(out)
    }
}
