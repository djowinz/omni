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

impl UploadStatus {
    /// Wire-format string used both by the Worker (response body) and by the
    /// editor (WS payload). Single source of truth for the enum ↔ string map.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Deduplicated => "deduplicated",
            Self::Updated => "updated",
            Self::Unchanged => "unchanged",
        }
    }

    /// Parse the Worker's response-body `status` field; unknown values map to
    /// `Created` (newly-accepted upload) since that's the default response.
    pub fn from_worker(s: &str) -> Self {
        match s {
            "deduplicated" => Self::Deduplicated,
            "updated" => Self::Updated,
            "unchanged" => Self::Unchanged,
            _ => Self::Created,
        }
    }
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
    let files_raw: BTreeMap<String, Vec<u8>> = match req.kind {
        ArtifactKind::Theme => read_theme(&req.source_path).await?,
        ArtifactKind::Bundle => walk_bundle(&req.source_path).await?,
    };
    let uncompressed_size: u64 = files_raw.values().map(|v| v.len() as u64).sum();

    let (manifest_kind, manifest_name) = match req.kind {
        ArtifactKind::Theme => ("theme", req.name.clone()),
        ArtifactKind::Bundle => ("bundle", req.name.clone()),
    };
    let manifest = build_manifest(req, &files_raw)?;

    // Sanitize pre-pack (invariant #7 — never skip). The bytes that the JWS
    // covers MUST be the exact bytes the Worker re-sanitizes and serves; that's
    // why pack_signed_bundle runs on the post-sanitize file set.
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

    let sanitized_manifest = rebuild_manifest_with(&manifest, &sanitized_files);
    let content_hash_bytes = omni_bundle::canonical_hash(&sanitized_manifest, &sanitized_files);
    let content_hash = hex::encode(content_hash_bytes);

    let sanitized_bytes =
        omni_identity::pack_signed_bundle(&sanitized_manifest, &sanitized_files, identity, limits)
            .map_err(|e| UploadError::BadInput {
                msg: "pack_signed_bundle failed".into(),
                source: Some(Box::new(e)),
            })?;

    let (thumbnail_png, sanitized_bytes) =
        render_thumbnail(req.kind, &sanitized_files, sanitized_bytes).await?;

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
    // Errors surface through the `Result` return path; the caller owns the
    // error envelope. `Done` still fires on success as the terminal marker.
    let result = upload_inner(req, guard, identity, client, progress.clone()).await?;
    let _ = progress
        .send(UploadProgress::Done {
            result: result.clone(),
        })
        .await;
    Ok(result)
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

    // Order matters: fetch server limits BEFORE pack_only, even though
    // pack_only validates against compile-time defaults first. Post-#011
    // thumbnail integration, pack_only runs real Ultralight render
    // (~500ms–1s of CPU/GPU); serializing behind the ~50ms limits fetch
    // avoids rendering thumbnails for oversized inputs that will be
    // rejected anyway. The cost profile inverted the concurrency
    // tradeoff #009 originally chose (try_join); re-evaluating it if
    // the thumbnail path becomes cheap again is valid future work.
    //
    // Only Network failures fall back to the compile-time default (with
    // a logged warning); any ServerReject (Auth/Admin/Malformed) must
    // surface verbatim — silently defaulting would let auth failures
    // masquerade as size-limit errors.
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

    let result = if let Some(id) = req.update_artifact_id.clone() {
        let manifest_value = serde_json::to_value(&pack.manifest).unwrap_or_default();
        client
            .patch(
                &id,
                super::client::PatchEdit {
                    manifest: Some(manifest_value),
                    bundle_bytes: Some(pack.sanitized_bytes),
                    thumbnail_bytes: Some(pack.thumbnail_png),
                },
            )
            .await?
    } else {
        client.upload(pack, progress.clone()).await?
    };

    Ok(result)
}

// --- helpers ---

/// Render a thumbnail for the sanitized artifact on a blocking worker. Both
/// paths are synchronous + CPU-heavy (Ultralight off-screen render), so we hop
/// onto `spawn_blocking` to avoid starving the async runtime. Thumbnail
/// generation happens inside the `Packing` phase window already emitted by the
/// caller — spec §10 floated a `GeneratingThumbnail` variant but sub-spec #009
/// closed `UploadProgress` without it, so we keep the existing vocabulary per
/// invariant #19.
///
/// For the bundle path, `sanitized_bytes` is `move`d into the closure and
/// returned back through the tuple so callers can reuse it without cloning the
/// MB-class buffer.
async fn render_thumbnail(
    kind: ArtifactKind,
    sanitized_files: &BTreeMap<String, Vec<u8>>,
    sanitized_bytes: Vec<u8>,
) -> Result<(Vec<u8>, Vec<u8>), UploadError> {
    use tracing::Instrument;
    let span = tracing::info_span!("thumbnail", kind = ?kind);
    render_thumbnail_inner(kind, sanitized_files, sanitized_bytes)
        .instrument(span)
        .await
}

async fn render_thumbnail_inner(
    kind: ArtifactKind,
    sanitized_files: &BTreeMap<String, Vec<u8>>,
    sanitized_bytes: Vec<u8>,
) -> Result<(Vec<u8>, Vec<u8>), UploadError> {
    match kind {
        ArtifactKind::Theme => {
            let css = sanitized_files
                .get("theme.css")
                .cloned()
                .ok_or_else(|| UploadError::BadInput {
                    msg: "sanitized theme missing theme.css".into(),
                    source: None,
                })?;
            let png = tokio::task::spawn_blocking(move || {
                crate::share::thumbnail::theme::generate_for_theme(
                    &css,
                    &crate::share::thumbnail::ThumbnailConfig::default(),
                )
            })
            .await
            .map_err(|e| UploadError::Io(std::io::Error::other(e)))?
            // Render-pipeline failure, not bad input — match the JoinError classification above for vocabulary consistency.
            .map_err(|e| UploadError::Io(std::io::Error::other(e)))?;
            Ok((png, sanitized_bytes))
        }
        ArtifactKind::Bundle => {
            // Move the buffer into the closure (no clone) and return it back
            // through the tuple so callers can still use the sanitized bytes.
            let (png_res, sanitized_bytes) = tokio::task::spawn_blocking(move || {
                let res = crate::share::thumbnail::bundle::generate_for_bundle(
                    &sanitized_bytes,
                    &crate::share::thumbnail::ThumbnailConfig::default(),
                );
                (res, sanitized_bytes)
            })
            .await
            .map_err(|e| UploadError::Io(std::io::Error::other(e)))?;
            // Render-pipeline failure, not bad input — match the JoinError classification above for vocabulary consistency.
            let png = png_res.map_err(|e| UploadError::Io(std::io::Error::other(e)))?;
            Ok((png, sanitized_bytes))
        }
    }
}

async fn read_theme(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, UploadError> {
    let path = path.to_path_buf();
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(path))
        .await
        .map_err(|e| UploadError::Io(std::io::Error::other(e)))??;
    let mut map = BTreeMap::new();
    map.insert("theme.css".to_string(), bytes);
    Ok(map)
}

async fn walk_bundle(root: &Path) -> Result<BTreeMap<String, Vec<u8>>, UploadError> {
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let mut out = BTreeMap::new();
        for entry in walkdir::WalkDir::new(&root).follow_links(false) {
            let entry = entry.map_err(|e| UploadError::Io(std::io::Error::other(e)))?;
            if entry.file_type().is_file() {
                let rel = entry
                    .path()
                    .strip_prefix(&root)
                    .unwrap_or(entry.path())
                    .to_string_lossy()
                    .replace('\\', "/");
                let bytes = std::fs::read(entry.path())?;
                out.insert(rel, bytes);
            }
        }
        Ok::<_, UploadError>(out)
    })
    .await
    .map_err(|e| UploadError::Io(std::io::Error::other(e)))?
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
    #[ignore = "requires Ultralight resources; run with --ignored after placing resources in target/debug/deps/"]
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
