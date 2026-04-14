use std::io;

/// Top-level error for the omni-bundle crate. Categories map to consumer
/// decisions: malformed = "bytes aren't a valid bundle"; unsafe = "structurally
/// valid but actively dangerous"; integrity = "doesn't match its own manifest";
/// io = "caller's environment failed". Third-party errors (zip, serde_json,
/// io) ride in the `#[source]` chain for diagnostic logs, not as public
/// variants.
#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("malformed bundle: {message}")]
    Malformed {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    },

    #[error("unsafe bundle: {kind:?} — {detail}")]
    Unsafe { kind: UnsafeKind, detail: String },

    #[error("integrity failure: {kind:?} — {detail}")]
    Integrity { kind: IntegrityKind, detail: String },

    #[error("io error ({kind:?}): {message}")]
    Io {
        kind: io::ErrorKind,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsafeKind {
    Path,
    PathTooDeep,
    PathTooLong,
    NonAscii,
    TooManyEntries,
    ZipBomb,
    SizeExceeded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrityKind {
    ManifestMissing,
    FileMissing,
    FileOrphan,
    HashMismatch,
    SchemaVersionUnsupported,
    DuplicatePath,
}

impl From<zip::result::ZipError> for BundleError {
    fn from(e: zip::result::ZipError) -> Self {
        BundleError::Malformed {
            message: format!("zip: {e}"),
            source: Some(Box::new(e)),
        }
    }
}

impl From<serde_json::Error> for BundleError {
    fn from(e: serde_json::Error) -> Self {
        BundleError::Malformed {
            message: format!("json: {e}"),
            source: Some(Box::new(e)),
        }
    }
}

impl From<io::Error> for BundleError {
    fn from(e: io::Error) -> Self {
        BundleError::Io {
            kind: e.kind(),
            message: e.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_formats() {
        let cases: Vec<BundleError> = vec![
            BundleError::Malformed {
                message: "bad zip".into(),
                source: None,
            },
            BundleError::Unsafe {
                kind: UnsafeKind::Path,
                detail: "../etc/passwd".into(),
            },
            BundleError::Integrity {
                kind: IntegrityKind::HashMismatch,
                detail: "themes/x.css".into(),
            },
            BundleError::Io {
                kind: io::ErrorKind::NotFound,
                message: "missing".into(),
            },
        ];
        for c in &cases {
            assert!(!format!("{c}").is_empty());
            if matches!(c, BundleError::Malformed { .. }) {
                use std::error::Error;
                let _ = c.source();
            }
        }
    }

    #[test]
    fn every_unsafe_kind_roundtrips() {
        let kinds = [
            UnsafeKind::Path,
            UnsafeKind::PathTooDeep,
            UnsafeKind::PathTooLong,
            UnsafeKind::NonAscii,
            UnsafeKind::TooManyEntries,
            UnsafeKind::ZipBomb,
            UnsafeKind::SizeExceeded,
        ];
        for k in kinds {
            let e = BundleError::Unsafe {
                kind: k,
                detail: "x".into(),
            };
            assert!(format!("{e}").contains(&format!("{k:?}")));
        }
    }

    #[test]
    fn every_integrity_kind_roundtrips() {
        let kinds = [
            IntegrityKind::ManifestMissing,
            IntegrityKind::FileMissing,
            IntegrityKind::FileOrphan,
            IntegrityKind::HashMismatch,
            IntegrityKind::SchemaVersionUnsupported,
            IntegrityKind::DuplicatePath,
        ];
        for k in kinds {
            let e = BundleError::Integrity {
                kind: k,
                detail: "x".into(),
            };
            assert!(format!("{e}").contains(&format!("{k:?}")));
        }
    }
}
