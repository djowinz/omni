//! Omni bundle/theme sanitization — public API stubs for Phase 0.
//!
//! All function bodies are `todo!()` and will be implemented in sub-spec 003.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizeVersion(pub u32);

pub const SANITIZE_VERSION: SanitizeVersion = SanitizeVersion(1);

#[derive(Debug, thiserror::Error)]
pub enum SanitizeError {
    #[error("zip structural error: {0}")]
    ZipStructural(String),
    #[error("manifest validation failed: {0}")]
    Manifest(String),
    #[error("font sanitization failed for {path}: {reason}")]
    Font { path: String, reason: String },
    #[error("image decode/encode failed for {path}: {reason}")]
    Image { path: String, reason: String },
    #[error("css parse failed for {path}: {reason}")]
    Css { path: String, reason: String },
    #[error("xml parse failed for {path}: {reason}")]
    Xml { path: String, reason: String },
    #[error("html sanitization failed for {path}: {reason}")]
    Html { path: String, reason: String },
    #[error("size limit exceeded: {kind}={actual} > {limit}")]
    SizeExceeded { kind: String, actual: u64, limit: u64 },
    #[error("path safety violation: {0}")]
    UnsafePath(String),
    #[error("zip bomb: compression ratio {ratio}:1 exceeds 100:1")]
    ZipBomb { ratio: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SanitizeReport {
    pub version: SanitizeVersion,
    pub original_size: u64,
    pub sanitized_size: u64,
    pub files: Vec<FileReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReport {
    pub path: String,
    pub kind: FileKind,
    pub original_sha256: [u8; 32],
    pub sanitized_sha256: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKind {
    Overlay,
    Css,
    Font,
    Image,
    Manifest,
}

pub fn sanitize_theme(_css_bytes: &[u8]) -> Result<(Vec<u8>, SanitizeReport), SanitizeError> {
    todo!("implemented in sub-spec 003")
}

pub fn sanitize_bundle(_zip_bytes: &[u8]) -> Result<(Vec<u8>, SanitizeReport), SanitizeError> {
    todo!("implemented in sub-spec 003")
}
