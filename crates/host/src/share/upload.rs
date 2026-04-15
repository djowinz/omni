//! Upload orchestration — pack → sanitize → sign → POST → cache.
//!
//! Per architectural invariant #1: `omni_identity::pack_signed_bundle` is the single
//! authority for signing; this module never imports `ed25519-dalek` or composes a
//! signature itself.
//!
//! Per invariant #7: every ingest path funnels through sanitize + verify — even
//! host-generated content. `pack_only` sanitizes (`omni_sanitize::sanitize_bundle`
//! / `sanitize_theme`) BEFORE signing so the bytes that the JWS covers are the
//! exact bytes the Worker will re-sanitize and serve.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use omni_bundle::{BundleLimits, FileEntry, Manifest, Tag};
use omni_guard_trait::Guard;
use omni_identity::Keypair;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use super::client::ShareClient;
use super::error::UploadError;
use super::progress::UploadProgress;

// Re-exported from this module (moved here in T8 from the W2 placeholder).
#[derive(Debug, Clone)]
pub struct UploadRequest {
    pub kind: ArtifactKind,
    pub source_path: PathBuf,
    pub name: String,
    pub description: String,
    pub tags: Vec<Tag>,
    pub license: String,
    pub version: Version,
    pub omni_min_version: Version,
    pub update_artifact_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Theme,
    Bundle,
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

/// Dry-run packing output (exposed to `upload.pack` WS message).
#[derive(Debug, Clone)]
pub struct PackResult {
    pub manifest: Manifest,
    pub manifest_name: String,
    pub manifest_kind: String, // "theme" | "bundle"
    pub sanitized_bytes: Vec<u8>,
    pub content_hash: String,
    pub thumbnail_png: Vec<u8>,
    pub compressed_size: u64,
    pub uncompressed_size: u64,
    pub sanitize_report: serde_json::Value,
}

/// Pack the source workspace path into a signed, sanitized bundle WITHOUT uploading.
/// Used both by `upload.pack` (dry-run) and by [`upload`] as step 1.
pub async fn pack_only(
    req: &UploadRequest,
    limits: &BundleLimits,
    identity: &Keypair,
) -> Result<PackResult, UploadError> {
    // 1. Read workspace inputs.
    let files_raw: BTreeMap<String, Vec<u8>> = match req.kind {
        ArtifactKind::Theme => read_theme(&req.source_path)?,
        ArtifactKind::Bundle => walk_bundle(&req.source_path)?,
    };
    let uncompressed_size: u64 = files_raw.values().map(|v| v.len() as u64).sum();

    // 2. Build manifest (per-file SHA-256 entries). Canonical hashing is inside omni-bundle.
    let (manifest_kind, manifest_name) = match req.kind {
        ArtifactKind::Theme => ("theme", req.name.clone()),
        ArtifactKind::Bundle => ("bundle", req.name.clone()),
    };
    let manifest = build_manifest(req, &files_raw)?;

    // 3. Sanitize pre-pack (invariant #7 — never skip).
    //    - Theme: `sanitize_theme(&bytes)`; single-file pipeline.
    //    - Bundle: `sanitize_bundle(&manifest, files)`; per-kind dispatch inside.
    let (sanitized_files, sanitize_report_value) = match req.kind {
        ArtifactKind::Theme => {
            let (_, css_bytes) = files_raw
                .iter()
                .next()
                .ok_or_else(|| UploadError::BadInput {
                    msg: "theme source missing".into(),
                    source: None,
                })?;
            let (out, report) =
                omni_sanitize::sanitize_theme(css_bytes).map_err(|e| UploadError::BadInput {
                    msg: "sanitize_theme failed".into(),
                    source: Some(Box::new(e)),
                })?;
            let mut map = BTreeMap::new();
            map.insert("theme.css".to_string(), out);
            (map, serde_json::to_value(report).unwrap_or_default())
        }
        ArtifactKind::Bundle => {
            let (out, report) = omni_sanitize::sanitize_bundle(&manifest, files_raw.clone())
                .map_err(|e| UploadError::BadInput {
                    msg: "sanitize_bundle failed".into(),
                    source: Some(Box::new(e)),
                })?;
            (out, serde_json::to_value(report).unwrap_or_default())
        }
    };

    // 4. Rebuild manifest over the sanitized bytes so per-file sha256s match
    //    what gets packed; pack_signed_bundle re-hashes and validates anyway.
    let sanitized_manifest = rebuild_manifest_with(&manifest, &sanitized_files);

    // 5. Canonical content hash over the sanitized manifest bytes
    //    (invariant #6: SHA-256(RFC 8785 JCS manifest bytes)). This is the
    //    dedup key the Worker indexes on.
    let content_hash_bytes = omni_bundle::canonical_hash(&sanitized_manifest, &sanitized_files);
    let content_hash = hex::encode(content_hash_bytes);

    // 6. pack_signed_bundle — single signing authority. Produces the .omnipkg
    //    bytes with `signature.jws` inside (invariant #6a).
    let sanitized_bytes = match req.kind {
        ArtifactKind::Theme => {
            // Themes ship as raw CSS bytes (worker-api §4.1); signing is over the
            // bundle path only. For theme upload we still pack+sign so the Worker
            // sees the JWS envelope — same as bundle but with a single-file manifest.
            omni_identity::pack_signed_bundle(
                &sanitized_manifest,
                &sanitized_files,
                identity,
                limits,
            )
            .map_err(|e| UploadError::BadInput {
                msg: "pack_signed_bundle (theme) failed".into(),
                source: Some(Box::new(e)),
            })?
        }
        ArtifactKind::Bundle => omni_identity::pack_signed_bundle(
            &sanitized_manifest,
            &sanitized_files,
            identity,
            limits,
        )
        .map_err(|e| UploadError::BadInput {
            msg: "pack_signed_bundle (bundle) failed".into(),
            source: Some(Box::new(e)),
        })?,
    };

    // 7. Thumbnail (delegated to sub-spec #011; placeholder 1×1 PNG until then).
    let thumbnail_png = placeholder_thumbnail();

    Ok(PackResult {
        manifest_name,
        manifest_kind: manifest_kind.into(),
        compressed_size: sanitized_bytes.len() as u64,
        uncompressed_size,
        manifest: sanitized_manifest,
        sanitized_bytes,
        content_hash,
        thumbnail_png,
        sanitize_report: sanitize_report_value,
    })
}

/// Full upload path. `guard` is probe-only (device_id, verify_self_integrity).
pub async fn upload(
    req: UploadRequest,
    guard: Arc<dyn Guard>,
    identity: Arc<Keypair>,
    client: Arc<ShareClient>,
    progress: mpsc::Sender<UploadProgress>,
) -> Result<UploadResult, UploadError> {
    // Top-level Err paths emit a final `UploadProgress::Error` frame before the
    // sender drops, so the editor sees a terminal progress event alongside the
    // returned `publishResult` error envelope. `Done` is still sent on success.
    match upload_inner(req, guard, identity, client, progress.clone()).await {
        Ok(result) => {
            let _ = progress
                .send(UploadProgress::Done {
                    result: result.clone(),
                })
                .await;
            Ok(result)
        }
        Err(e) => {
            let _ = progress
                .send(UploadProgress::Error {
                    code: e.code().to_string(),
                    message: e.user_message(),
                })
                .await;
            Err(e)
        }
    }
}

async fn upload_inner(
    req: UploadRequest,
    guard: Arc<dyn Guard>,
    identity: Arc<Keypair>,
    client: Arc<ShareClient>,
    progress: mpsc::Sender<UploadProgress>,
) -> Result<UploadResult, UploadError> {
    // 0. Self-integrity gate (invariant #9 style).
    guard
        .verify_self_integrity()
        .map_err(|e| UploadError::Integrity {
            msg: "self-integrity check failed".into(),
            source: Some(Box::new(e)),
        })?;

    let _ = progress.send(UploadProgress::Packing).await;
    // Propagate config_limits errors. Only Network failures fall back to the
    // compile-time default with a logged warning; any ServerReject (Auth/Admin/
    // Malformed) must surface verbatim — silently defaulting would let auth
    // failures masquerade as size-limit errors.
    let limits = match client.config_limits().await {
        Ok(l) => l,
        Err(UploadError::Network(e)) => {
            tracing::warn!(
                error = %e,
                "config_limits network failure; falling back to BundleLimits::DEFAULT"
            );
            BundleLimits::DEFAULT
        }
        Err(e) => return Err(e),
    };
    let pack = pack_only(&req, &limits, &identity).await?;

    if pack.sanitized_bytes.len() as u64 > limits.max_bundle_compressed {
        return Err(UploadError::BadInput {
            msg: format!(
                "bundle is {} bytes; server limit is {}",
                pack.sanitized_bytes.len(),
                limits.max_bundle_compressed
            ),
            source: None,
        });
    }

    let _ = progress
        .send(UploadProgress::Sanitizing {
            file: pack.manifest_name.clone(),
        })
        .await;

    // Upload (ShareClient owns JWS middleware + retry)
    let result = if let Some(id) = req.update_artifact_id.clone() {
        client
            .patch(
                &id,
                super::client::PatchEdit {
                    manifest: Some(serde_json::to_value(&pack.manifest).unwrap_or_default()),
                    bundle_bytes: Some(pack.sanitized_bytes.clone()),
                    thumbnail_bytes: Some(pack.thumbnail_png.clone()),
                },
            )
            .await?
    } else {
        client.upload(pack, progress.clone()).await?
    };

    Ok(result)
}

// --- helpers ---

fn read_theme(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, UploadError> {
    let bytes = std::fs::read(path)?;
    let mut map = BTreeMap::new();
    map.insert("theme.css".to_string(), bytes);
    Ok(map)
}

fn walk_bundle(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, UploadError> {
    let mut out = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false) {
        let entry = entry.map_err(|e| UploadError::Io(std::io::Error::other(e)))?;
        if entry.file_type().is_file() {
            let rel = entry
                .path()
                .strip_prefix(root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(entry.path())?;
            out.insert(rel, bytes);
        }
    }
    Ok(out)
}

fn build_manifest(
    req: &UploadRequest,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<Manifest, UploadError> {
    let entries: Vec<FileEntry> = files
        .iter()
        .map(|(path, bytes)| FileEntry {
            path: path.clone(),
            sha256: Sha256::digest(bytes).into(),
        })
        .collect();

    let entry_overlay = match req.kind {
        ArtifactKind::Theme => "theme.css".to_string(),
        ArtifactKind::Bundle => files
            .keys()
            .find(|k| k.ends_with("overlay.omni") || k.ends_with(".omni"))
            .cloned()
            .unwrap_or_else(|| "overlay.omni".to_string()),
    };

    Ok(Manifest {
        schema_version: 1,
        name: req.name.clone(),
        version: req.version.clone(),
        omni_min_version: req.omni_min_version.clone(),
        description: req.description.clone(),
        tags: req.tags.clone(),
        license: req.license.clone(),
        entry_overlay,
        default_theme: None,
        sensor_requirements: vec![],
        files: entries,
        resource_kinds: None,
    })
}

/// Rebuild a manifest with fresh per-file sha256s taken from `files`. The
/// structural fields (name, version, tags, …) are preserved from `base`.
fn rebuild_manifest_with(base: &Manifest, files: &BTreeMap<String, Vec<u8>>) -> Manifest {
    let mut m = base.clone();
    let mut entries: Vec<FileEntry> = files
        .iter()
        .map(|(path, bytes)| FileEntry {
            path: path.clone(),
            sha256: Sha256::digest(bytes).into(),
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    // Entry overlay must still resolve; if the sanitized file set lost it, fall
    // back to the first file so the manifest stays structurally valid. Handler
    // parsers in `omni-sanitize` do not rename files, so this is belt-and-suspenders.
    if !entries.iter().any(|e| e.path == m.entry_overlay) {
        if let Some(first) = entries.first() {
            m.entry_overlay = first.path.clone();
        }
    }
    m.files = entries;
    m
}

fn placeholder_thumbnail() -> Vec<u8> {
    // 1×1 transparent PNG (89 bytes); sub-spec #011 replaces with a real
    // rendered thumbnail once the thumbnail API lands.
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn types_are_send_sync() {
        assert_send_sync::<UploadRequest>();
        assert_send_sync::<UploadResult>();
        assert_send_sync::<UploadStatus>();
        assert_send_sync::<ArtifactKind>();
        assert_send_sync::<PackResult>();
    }

    #[test]
    fn upload_result_serde_roundtrip() {
        let r = UploadResult {
            artifact_id: "abc".into(),
            content_hash: "deadbeef".into(),
            r2_url: "https://r2/x".into(),
            thumbnail_url: "https://r2/thumb".into(),
            status: UploadStatus::Created,
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: UploadResult = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn upload_status_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&UploadStatus::Deduplicated).unwrap(),
            "\"deduplicated\""
        );
        let back: UploadStatus = serde_json::from_str("\"unchanged\"").unwrap();
        assert_eq!(back, UploadStatus::Unchanged);
    }

    #[tokio::test]
    async fn pack_only_theme_roundtrips() {
        // Keypair::generate is the test-friendly constructor in omni-identity.
        let kp = Keypair::generate();
        let tmp = tempfile::NamedTempFile::new().expect("tmp");
        std::fs::write(tmp.path(), b":root { --x: 1; }").expect("write");
        let req = UploadRequest {
            kind: ArtifactKind::Theme,
            source_path: tmp.path().to_path_buf(),
            name: "t".into(),
            description: String::new(),
            tags: vec![],
            license: "MIT".into(),
            version: "1.0.0".parse().unwrap(),
            omni_min_version: "0.1.0".parse().unwrap(),
            update_artifact_id: None,
        };
        let pack = pack_only(&req, &BundleLimits::DEFAULT, &kp)
            .await
            .expect("pack_only theme");
        assert!(pack.compressed_size > 0);
        assert_eq!(pack.content_hash.len(), 64);
        assert_eq!(pack.manifest_kind, "theme");
    }
}
