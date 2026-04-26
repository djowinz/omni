//! Upload orchestration — pack → sanitize → sign → POST → cache.
//!
//! Per architectural invariant #1: `identity::pack_signed_bundle` is the single
//! authority for signing; this module never imports `ed25519-dalek` or composes a
//! signature itself.
//!
//! Per invariant #7: every ingest path funnels through sanitize + verify — even
//! host-generated content. `pack_only` sanitizes (`sanitize::sanitize_bundle`
//! / `sanitize_theme`) BEFORE signing so the bytes that the JWS covers are the
//! exact bytes the Worker will re-sanitize and serve.
//!
//! ## Pack stages (upload-flow redesign Wave A1 / OWI-40)
//!
//! `pack_only_with_progress` emits five `PackProgress` frames in order:
//! `Schema → ContentSafety → Asset → Dependency → Size`. Each frame fires
//! `Running` on entry and `Passed` on success, or `Failed` with a freeform
//! `detail` string on the first stage that errors. Dependency Check is the
//! one stage that accumulates ALL violations across categories before
//! failing (INV-7.3.7); every other stage fail-fast.
//!
//! Stage ↔ implementation mapping:
//!
//! * `Schema` → `build_manifest` (manifest construction validates structural
//!   prerequisites + populates `resource_kinds`).
//! * `ContentSafety` → `sanitize::sanitize_bundle` first half (CSS URL
//!   whitelist, overlay XML structural gate, font magic). The host runs
//!   sanitize as a single call; the ContentSafety + Asset stages are emitted
//!   around that call as conceptual checkpoints.
//! * `Asset` → `sanitize::sanitize_bundle` second half (image re-decode +
//!   PNG re-encode, font ttf-parser check).
//! * `Dependency` → `dep_resolver::resolve` (missing-refs + unused-files).
//!   Wave B1.5 / OWI-54 adds the third category (content-safety / NSFW per
//!   bundled image).
//! * `Size` → the existing `pack.sanitized_bytes.len() vs
//!   limits.max_bundle_compressed` comparison (today this check lives in
//!   `upload_inner`; the dry-run Step 3 surface re-runs it inside
//!   `pack_only_with_progress` so the renderer's Step 3 progress UI advances
//!   all five stages even on dry-run).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bundle::{BundleLimits, FileEntry, Manifest, Tag};
use identity::Keypair;
use omni_guard_trait::Guard;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

use super::client::ShareClient;
use super::dep_resolver::{self, Violation};
use super::error::{DependencyViolationDetail, UploadError};
use super::progress::UploadProgress;
use super::ws_messages::{PackProgress, PackStage, StageStatus};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct UploadResult {
    pub artifact_id: String,
    pub content_hash: String,
    pub r2_url: String,
    pub thumbnail_url: String,
    pub status: UploadStatus,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
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

/// Sink for `PackProgress` frames emitted by `pack_only_with_progress`. The
/// dispatcher in `share::ws_messages::handle_pack` hands an `mpsc::Sender`
/// wrapped in this trait object; the dry-run code path in `upload_inner`
/// passes `None` so the ambient `UploadProgress` channel keeps owning
/// progress for full publishes.
///
/// Contract: implementors must NOT block — frames are emitted from the
/// pack pipeline's hot path. Channel-full means a slow consumer; drop the
/// frame rather than awaiting.
pub trait PackProgressSink: Send + Sync {
    fn emit(&self, frame: PackProgress);
}

/// `mpsc::Sender<PackProgress>` adapter. `try_send` so a slow consumer can't
/// block the pack pipeline.
impl PackProgressSink for mpsc::Sender<PackProgress> {
    fn emit(&self, frame: PackProgress) {
        let _ = self.try_send(frame);
    }
}

/// Backwards-compatible thin wrapper. Delegates to
/// [`pack_only_with_progress`] with no progress sink. Kept as the public
/// entry point so existing callers (`upload_inner`, ws_messages handler)
/// stay source-compatible.
pub async fn pack_only(
    req: &UploadRequest,
    limits: &BundleLimits,
    identity: &Keypair,
) -> Result<PackResult, UploadError> {
    pack_only_with_progress(req, limits, identity, None).await
}

/// Pack the source workspace path into a signed, sanitized bundle WITHOUT
/// uploading. Used by `upload.pack` (dry-run, with `progress = Some(...)`)
/// and by [`upload`] as step 1 (`progress = None` — `upload_inner` owns its
/// own `UploadProgress` lifecycle).
///
/// `progress` is the Step 3 packing-stage emitter (spec §8.8 + INV-7.3.\*).
/// When `Some`, frames fire in order at each stage transition; when `None`,
/// no frames are emitted and the pipeline behaves identically to the
/// pre-OWI-40 `pack_only` (publish path stays unchanged).
///
/// On Dependency Check failure, returns
/// [`UploadError::DependencyViolations`] carrying every accumulated
/// violation — never just the first (INV-7.3.7).
pub async fn pack_only_with_progress(
    req: &UploadRequest,
    limits: &BundleLimits,
    identity: &Keypair,
    progress: Option<&dyn PackProgressSink>,
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

    // ── Stage 1: Schema (manifest construction) ──────────────────────────
    emit_stage(progress, PackStage::Schema, StageStatus::Running, None);
    let manifest = match build_manifest(req, &files_raw) {
        Ok(m) => m,
        Err(e) => {
            emit_stage(
                progress,
                PackStage::Schema,
                StageStatus::Failed,
                Some(format!("{e}")),
            );
            return Err(e);
        }
    };
    emit_stage(progress, PackStage::Schema, StageStatus::Passed, None);

    // ── Stage 2 + 3: ContentSafety + Asset ───────────────────────────────
    // Sanitize is a single call internally; the two stages are conceptual
    // checkpoints around it. ContentSafety covers CSS-URL whitelist + XML
    // structural gate; Asset covers image re-decode + font ttf check.
    // A sanitize failure can map to either stage depending on which handler
    // raised it, but without parsing the SanitizeError detail we
    // conservatively attribute the failure to ContentSafety (the first of
    // the two stages) and skip Asset.
    //
    // Sanitize pre-pack (invariant #7 — never skip). The bytes that the JWS
    // covers MUST be the exact bytes the Worker re-sanitizes and serves;
    // that's why pack_signed_bundle runs on the post-sanitize file set.
    emit_stage(
        progress,
        PackStage::ContentSafety,
        StageStatus::Running,
        None,
    );
    let (sanitized_files, sanitize_report_value) = match req.kind {
        ArtifactKind::Theme => {
            let (_, css_bytes) = files_raw
                .iter()
                .next()
                .ok_or_else(|| UploadError::BadInput {
                    msg: "theme source missing".into(),
                    source: None,
                })?;
            match sanitize::sanitize_theme(css_bytes) {
                Ok((out, report)) => {
                    let mut map = BTreeMap::new();
                    map.insert("theme.css".to_string(), out);
                    (map, serde_json::to_value(report).unwrap_or_default())
                }
                Err(e) => {
                    emit_stage(
                        progress,
                        PackStage::ContentSafety,
                        StageStatus::Failed,
                        Some(format!("{e}")),
                    );
                    return Err(UploadError::BadInput {
                        // Display includes the SanitizeError variant + inner detail;
                        // without it the wire envelope just says "sanitize_theme failed"
                        // and the operator has no idea which file / which limit tripped.
                        msg: format!("sanitize_theme failed: {e}"),
                        source: Some(Box::new(e)),
                    });
                }
            }
        }
        ArtifactKind::Bundle => {
            match sanitize::sanitize_bundle(&manifest, files_raw.clone()) {
                Ok((out, report)) => (out, serde_json::to_value(report).unwrap_or_default()),
                Err(e) => {
                    emit_stage(
                        progress,
                        PackStage::ContentSafety,
                        StageStatus::Failed,
                        Some(format!("{e}")),
                    );
                    return Err(UploadError::BadInput {
                        // Display the SanitizeError so the user sees which file /
                        // which limit tripped. Without this the wire only carries
                        // "sanitize_bundle failed" and the rejection is opaque.
                        msg: format!("sanitize_bundle failed: {e}"),
                        source: Some(Box::new(e)),
                    });
                }
            }
        }
    };
    emit_stage(progress, PackStage::ContentSafety, StageStatus::Passed, None);
    // Asset stage piggybacks on the same sanitize call (image re-decode +
    // font ttf check live inside `sanitize_bundle`). If we got here,
    // sanitize succeeded → Asset trivially passes.
    emit_stage(progress, PackStage::Asset, StageStatus::Running, None);
    emit_stage(progress, PackStage::Asset, StageStatus::Passed, None);

    // ── Stage 4: Dependency (missing-refs + unused-files) ────────────────
    // Theme uploads (single CSS file, no referenced images/fonts) skip the
    // resolver — there's nothing to resolve. Bundles run the full walk.
    emit_stage(progress, PackStage::Dependency, StageStatus::Running, None);
    if matches!(req.kind, ArtifactKind::Bundle) {
        let resolution = dep_resolver::resolve(&sanitized_files).map_err(|e| {
            emit_stage(
                progress,
                PackStage::Dependency,
                StageStatus::Failed,
                Some(format!("{e}")),
            );
            UploadError::BadInput {
                msg: format!("dep_resolver: {e}"),
                source: None,
            }
        })?;
        if !resolution.violations.is_empty() {
            let violations: Vec<DependencyViolationDetail> = resolution
                .violations
                .iter()
                .map(violation_to_wire)
                .collect();
            emit_stage(
                progress,
                PackStage::Dependency,
                StageStatus::Failed,
                Some(format!(
                    "{} dependency violation{}",
                    violations.len(),
                    if violations.len() == 1 { "" } else { "s" }
                )),
            );
            return Err(UploadError::DependencyViolations { violations });
        }
    }
    emit_stage(progress, PackStage::Dependency, StageStatus::Passed, None);

    // ── Sign + thumbnail (no progress stage of their own) ────────────────
    let sanitized_manifest = rebuild_manifest_with(&manifest, &sanitized_files);
    let content_hash_bytes = bundle::canonical_hash(&sanitized_manifest, &sanitized_files);
    let content_hash = hex::encode(content_hash_bytes);

    let sanitized_bytes =
        identity::pack_signed_bundle(&sanitized_manifest, &sanitized_files, identity, limits)
            .map_err(|e| UploadError::BadInput {
                msg: "pack_signed_bundle failed".into(),
                source: Some(Box::new(e)),
            })?;

    let (thumbnail_png, sanitized_bytes) =
        render_thumbnail(req.kind, &sanitized_files, sanitized_bytes).await?;

    let compressed_size = sanitized_bytes.len() as u64;

    // ── Stage 5: Size (vs server-resolved limit) ─────────────────────────
    emit_stage(progress, PackStage::Size, StageStatus::Running, None);
    if compressed_size > limits.max_bundle_compressed {
        let detail = format!(
            "bundle is {compressed_size} bytes; server limit is {}",
            limits.max_bundle_compressed
        );
        emit_stage(
            progress,
            PackStage::Size,
            StageStatus::Failed,
            Some(detail.clone()),
        );
        return Err(UploadError::BadInput {
            msg: detail,
            source: None,
        });
    }
    emit_stage(progress, PackStage::Size, StageStatus::Passed, None);

    Ok(PackResult {
        manifest_name,
        manifest_kind: manifest_kind.into(),
        compressed_size,
        uncompressed_size,
        manifest: sanitized_manifest,
        sanitized_bytes,
        content_hash,
        thumbnail_png,
        sanitize_report: sanitize_report_value,
    })
}

/// Wire-shape conversion from the resolver's internal `Violation` enum to
/// the `DependencyViolationDetail` struct that crosses the WS boundary.
/// Keeps the resolver crate-internal and the wire shape decoupled from its
/// internals — Wave B1.5 / OWI-54 will add the `ContentSafety` arm here
/// when the moderator lands.
fn violation_to_wire(v: &Violation) -> DependencyViolationDetail {
    match v {
        Violation::MissingRef { path } => DependencyViolationDetail {
            kind: "missing-ref".into(),
            path: path.clone(),
            detail: None,
        },
        Violation::UnusedFile { path } => DependencyViolationDetail {
            kind: "unused-file".into(),
            path: path.clone(),
            detail: None,
        },
    }
}

/// Helper that no-ops when no sink is installed. Centralizes the
/// `if let Some(sink) = progress` boilerplate so the stage call sites stay
/// readable.
fn emit_stage(
    progress: Option<&dyn PackProgressSink>,
    stage: PackStage,
    status: StageStatus,
    detail: Option<String>,
) {
    if let Some(sink) = progress {
        sink.emit(PackProgress {
            stage,
            status,
            detail,
        });
    }
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
            let css =
                sanitized_files
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
        // Skip hidden entries (name starts with '.') — this excludes host-owned
        // runtime scratch like `.omni_current.html` (see `ul_renderer::SCRATCH_NAME`)
        // plus any `.bak`/`.swp`/`.git*` files a user might leave in the overlay
        // dir. Without this filter the sanitizer rejects the bundle because those
        // files have no handler kind and exceed per-file limits.
        let walker = walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                // depth 0 is the root itself — always traverse. Filtering the
                // root out would empty the walk entirely (and on Windows,
                // tempfile::tempdir() names start with `.tmp`, so the test
                // would false-positive that regression).
                if entry.depth() == 0 {
                    return true;
                }
                entry
                    .file_name()
                    .to_str()
                    .map(|n| !n.starts_with('.'))
                    .unwrap_or(true)
            });
        for entry in walker {
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

    // Populate `resource_kinds` from the file paths so the Worker can correctly
    // classify the upload as bundle-vs-theme-only (spec §8.7 / OWI-33). Before
    // this fix, every upload shipped `resource_kinds: None`, and the Worker's
    // `isThemeOnly()` defaulted absent maps to true — biasing every bundle into
    // the theme bucket.
    //
    // Mapping (matches the kind names declared by the shipped sanitize
    // handlers — `theme`, `font`, `image`, `overlay`):
    //   * `overlay.omni`            → "overlay"
    //   * `*.css`                   → "theme"
    //   * `fonts/*`                 → "font"
    //   * `images/*`                → "image"
    //
    // Each declared kind carries the same (`dir`, `extensions`, `max_size_bytes`)
    // shape that the shipped sanitize handlers use as defaults so dispatch in
    // `omni_sanitize::handlers::dispatch_for_path` keeps resolving the file to
    // the correct handler. If no file falls into any bucket, return `None` so
    // the Worker still receives a structurally minimal manifest.
    let mut resource_kinds: BTreeMap<String, bundle::ResourceKind> = BTreeMap::new();
    for path in files.keys() {
        let kind: Option<&'static str> = if path == "overlay.omni" {
            Some("overlay")
        } else if path.ends_with(".css") {
            Some("theme")
        } else if path.starts_with("fonts/") {
            Some("font")
        } else if path.starts_with("images/") {
            Some("image")
        } else {
            None
        };
        if let Some(k) = kind {
            // Idempotent: each kind only needs one declaration regardless of
            // how many matching files the bundle carries. The values below
            // mirror the shipped handler defaults in `crates/sanitize/src/handlers/`.
            resource_kinds.entry(k.into()).or_insert_with(|| match k {
                "overlay" => bundle::ResourceKind {
                    dir: String::new(),
                    extensions: vec!["omni".into()],
                    max_size_bytes: 131_072,
                },
                "theme" => bundle::ResourceKind {
                    dir: "themes".into(),
                    extensions: vec!["css".into()],
                    max_size_bytes: 131_072,
                },
                "font" => bundle::ResourceKind {
                    dir: "fonts".into(),
                    extensions: vec!["ttf".into(), "otf".into(), "woff2".into()],
                    max_size_bytes: 1_572_864,
                },
                "image" => bundle::ResourceKind {
                    dir: "images".into(),
                    extensions: vec!["png".into(), "jpg".into(), "jpeg".into(), "webp".into()],
                    max_size_bytes: 1_572_864,
                },
                _ => unreachable!("kind matched above"),
            });
        }
    }
    let resource_kinds = if resource_kinds.is_empty() {
        None
    } else {
        Some(resource_kinds)
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
        resource_kinds,
    })
}

/// Test-only wrapper exposing `build_manifest` to the integration test crate
/// at `crates/host/tests/build_manifest_resource_kinds.rs` (spec §8.7 / OWI-33).
///
/// `#[cfg(test)]` is intentionally NOT used here: integration tests live in a
/// separate compilation unit that links to the production library, so a
/// `cfg(test)`-gated symbol is invisible to them. The `_for_test` suffix marks
/// intent for human readers; production callers always use `build_manifest`
/// internally via `pack_only`.
#[doc(hidden)]
pub fn build_manifest_for_test(
    req: &UploadRequest,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<Manifest, UploadError> {
    build_manifest(req, files)
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
    async fn walk_bundle_skips_dotfiles() {
        // Regression: host-owned runtime scratch files like
        // `.omni_current.html` (written by `ul_renderer`) live inside every
        // overlay dir. Before this filter, walk_bundle picked them up and
        // sanitize_bundle rejected the whole upload because `.html` has no
        // handler kind. Skipping any `.`-prefixed entry fixes both that bug
        // and future scratch files the renderer might add.
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("overlay.omni"),
            b"<widget><template><div/></template></widget>",
        )
        .expect("write overlay");
        std::fs::write(tmp.path().join(".omni_current.html"), b"<html/>").expect("write scratch");
        std::fs::write(tmp.path().join(".swp"), b"editor swap").expect("write swp");
        let files = walk_bundle(tmp.path()).await.expect("walk");
        let names: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            vec!["overlay.omni"],
            "only the overlay survives the dotfile filter; got {names:?}"
        );
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
