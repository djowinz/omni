use crate::MAX_ENTRIES;

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("manifest missing from bundle")]
    ManifestMissing,
    #[error("file listed in manifest not present in zip: {0}")]
    FileMissing(String),
    #[error("file in zip not listed in manifest: {0}")]
    FileOrphan(String),
    #[error("hash mismatch for {path}: manifest={manifest}, actual={actual}")]
    HashMismatch { path: String, manifest: String, actual: String },
    #[error("size limit exceeded: {kind}={actual} > {limit}")]
    SizeExceeded { kind: String, actual: u64, limit: u64 },
    #[error("unsafe path: {0}")]
    UnsafePath(String),
    #[error("too many entries: {actual} > {limit}", limit = MAX_ENTRIES)]
    TooManyEntries { actual: usize },
    #[error("zip bomb: compression ratio {0}:1 exceeds 100:1")]
    ZipBomb(u64),
    #[error("tag not in controlled vocabulary: {0}")]
    InvalidTag(String),
    #[error("invalid semver: {0}")]
    InvalidVersion(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_variant_formats() {
        let cases: Vec<BundleError> = vec![
            BundleError::ManifestMissing,
            BundleError::FileMissing("x".into()),
            BundleError::FileOrphan("y".into()),
            BundleError::HashMismatch {
                path: "a".into(),
                manifest: "aa".into(),
                actual: "bb".into(),
            },
            BundleError::SizeExceeded { kind: "font".into(), actual: 99, limit: 10 },
            BundleError::UnsafePath("../x".into()),
            BundleError::TooManyEntries { actual: 100 },
            BundleError::ZipBomb(500),
            BundleError::InvalidTag("foo".into()),
            BundleError::InvalidVersion("1".into()),
        ];
        for c in cases {
            let s = format!("{c}");
            assert!(!s.is_empty());
        }
    }
}
