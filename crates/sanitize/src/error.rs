//! Error taxonomy for omni-sanitize (retro-005 D9 / invariant #19a: categorized, no `#[from]`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SanitizeVersion(pub u32);

pub const SANITIZE_VERSION: SanitizeVersion = SanitizeVersion(1);

#[derive(Debug, thiserror::Error)]
pub enum SanitizeError {
    #[error("malformed: {message}")]
    Malformed {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    },

    #[error("rejected executable magic {prefix_hex} at {path}")]
    RejectedExecutableMagic { prefix_hex: String, path: String },

    #[error("unknown resource kind '{kind}' (supported: {supported:?})")]
    UnknownResourceKind {
        kind: String,
        supported: Vec<&'static str>,
    },

    #[error("handler error for {path} (kind={kind}): {detail}")]
    Handler {
        kind: &'static str,
        path: String,
        detail: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    },

    #[error("size exceeded at {path}: {actual} > {limit}")]
    SizeExceeded {
        path: String,
        actual: u64,
        limit: u64,
    },
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
    Theme,
    Font,
    Image,
    Overlay,
}
