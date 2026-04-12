//! Omni bundle manifest + pack/unpack — public API stubs for Phase 0.
//!
//! Bodies are `todo!()`; implemented in sub-spec 005.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ---------- Size / structural limits (authoritative) ----------

pub const MAX_BUNDLE_COMPRESSED: u64 = 5 * 1024 * 1024;
pub const MAX_BUNDLE_UNCOMPRESSED: u64 = 10 * 1024 * 1024;
pub const MAX_FONT: u64 = 1_572_864;
pub const MAX_IMAGE_RAW: u64 = 1_572_864;
pub const MAX_IMAGE_REENCODED: u64 = 1_048_576;
pub const MAX_CSS: u64 = 131_072;
pub const MAX_OVERLAY: u64 = 131_072;
pub const MAX_THEME_ONLY: u64 = 65_536;
pub const MAX_ENTRIES: usize = 32;
pub const MAX_PATH_DEPTH: usize = 2;
pub const MAX_COMPRESSION_RATIO: u64 = 100;

// ---------- Controlled tag vocabulary ----------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tag {
    Dark,
    Light,
    Minimal,
    Gaming,
    Neon,
    Retro,
    Cyberpunk,
    Pastel,
    HighContrast,
    Monospace,
    Racing,
    Flightsim,
    Mmo,
    Fps,
    Productivity,
    Creative,
}

// ---------- Manifest types ----------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub name: String,
    pub version: semver::Version,
    pub omni_min_version: semver::Version,
    pub description: String,
    pub tags: Vec<Tag>,
    pub license: String,
    pub entry_overlay: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_theme: Option<String>,
    #[serde(default)]
    pub sensor_requirements: Vec<String>,
    pub files: Vec<FileEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    #[serde(with = "hex_sha256")]
    pub sha256: [u8; 32],
}

// ---------- Errors ----------

#[derive(Debug, thiserror::Error)]
pub enum BundleError {
    #[error("zip error: {0}")]
    Zip(String),
    #[error("manifest missing or invalid: {0}")]
    Manifest(String),
    #[error("file not found in bundle: {0}")]
    MissingFile(String),
    #[error("hash mismatch for {path}")]
    HashMismatch { path: String },
    #[error("size limit exceeded: {kind}={actual} > {limit}")]
    SizeExceeded { kind: String, actual: u64, limit: u64 },
    #[error("too many entries: {0} > 32")]
    TooManyEntries(usize),
    #[error("unsafe path: {0}")]
    UnsafePath(String),
    #[error("io: {0}")]
    Io(String),
}

// ---------- Public API ----------

pub fn pack(
    _manifest: &Manifest,
    _files: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, BundleError> {
    todo!("implemented in sub-spec 005")
}

pub fn unpack(
    _zip_bytes: &[u8],
) -> Result<(Manifest, BTreeMap<String, Vec<u8>>), BundleError> {
    todo!("implemented in sub-spec 005")
}

pub fn canonical_hash(
    _manifest: &Manifest,
    _files: &BTreeMap<String, Vec<u8>>,
) -> [u8; 32] {
    todo!("implemented in sub-spec 005")
}

// ---------- Internal helpers ----------

mod hex_sha256 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8; 32], s: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(64);
        for b in bytes {
            out.push_str(&format!("{:02x}", b));
        }
        s.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u8; 32], D::Error> {
        let s = String::deserialize(d)?;
        if s.len() != 64 {
            return Err(serde::de::Error::custom("sha256 hex must be 64 chars"));
        }
        let mut out = [0u8; 32];
        for i in 0..32 {
            out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(out)
    }
}
