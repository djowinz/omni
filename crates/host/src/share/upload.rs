//! Upload orchestration — pack → sanitize → sign → POST → cache.
//!
//! The full `pack_only` / `upload` orchestration lands in T8 of plan #009.
//! This file currently exposes only the types that sibling modules
//! (`progress`, `client`) forward-reference so the crate links.

use omni_bundle::Manifest;
use serde::{Deserialize, Serialize};

/// Output of `pack_only` — buffered bytes + manifest ready for upload.
/// The complete field set lands in T8; these are the fields `ShareClient::upload`
/// reads today.
#[derive(Debug, Clone)]
pub struct PackResult {
    pub manifest: Manifest,
    pub sanitized_bytes: Vec<u8>,
    pub thumbnail_png: Vec<u8>,
    pub content_hash: String,
    pub manifest_name: String,
    pub manifest_kind: String, // "theme" | "bundle"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UploadResult {
    pub artifact_id: String,
    pub content_hash: String,
    pub r2_url: String,
    pub thumbnail_url: String,
    pub status: UploadStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UploadStatus {
    Created,
    Deduplicated,
    Updated,
    Unchanged,
}
