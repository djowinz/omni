//! WebSocket message handlers for `upload.*` / `identity.*` / `config.*` / `report.submit`.
//! Wire shapes are authoritative in ws-explorer.md — do not invent fields here.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use arc_swap::ArcSwap;
use bundle::{BundleLimits, Tag};
use identity::{Keypair, PublicKey};
use omni_guard_trait::Guard;
use semver::Version;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::client::{ListParams, ReportBody, ShareClient};
use super::error::UploadError;
use super::handlers::{
    self, install_outcome_to_result_frame, install_progress_to_contract_frame, map_preview_error,
    ErrorPayload,
};
use super::identity_metadata::IdentityMetadata;
use super::install::{self, InstallProgress, InstallRequest};
use super::preview::{PreviewSlot, ThemeSwap};
use super::progress::{error_envelope, pump_to_ws};
use super::registry::{RegistryHandle, RegistryKind};
use super::sidecar::{read_sidecar, read_theme_sidecar, PublishSidecar};
use super::tofu::TofuStore;
use super::upload::{pack_only_with_progress, upload, ArtifactKind, UploadRequest};
use crate::omni::parser::parse_omni_with_diagnostics;
use crate::workspace::structure::{list_overlays, list_themes};

/// Bundle of shared state consumed by `explorer.*` + `upload.*` + `identity.*`
/// + `config.*` + `report.*` WebSocket handlers.
///
/// The original three fields (`identity`, `guard`, `client`) shipped with
/// sub-spec #009 to drive the upload-family handlers. Sub-spec #021 extends
/// the bundle with the install/preview deps enumerated below so the async
/// bridge can drive `explorer.install|preview|cancelPreview|list|get` through
/// the same `ShareContext` surface. All new fields are `Arc`-wrapped so the
/// async handler futures (spawned via `share_runtime.spawn(...)`) can capture
/// them without consuming `ShareContext`; fields that mutate across requests
/// use interior `Mutex`.
pub struct ShareContext {
    /// Atomic swap slot for the signing keypair. Shared *by clone* with
    /// the embedded `ShareClient` — both fields hold the same outer
    /// `Arc<ArcSwap<Keypair>>`, so a single `.store(new_kp)` from the
    /// rotate handler retargets every signer (HTTP-JWS for upload/install
    /// and any future signer routed through `ShareClient`) without
    /// re-threading the keypair through long-lived spawned futures.
    ///
    /// Read sites (`handle_pack`, `handle_publish`, `handle_identity_show`,
    /// `handle_identity_backup`) do `.load()` to obtain a `Guard<Arc<Keypair>>`
    /// that derefs to `&Keypair` — they pass that wherever the underlying
    /// API takes `&Keypair`. Sites that need an owned `Arc<Keypair>`
    /// (today: `super::upload::upload`) call `.load_full()` to clone the
    /// inner Arc out of the swap.
    pub identity: Arc<ArcSwap<Keypair>>,
    pub guard: Arc<dyn Guard>,
    pub client: Arc<ShareClient>,

    /// TOFU fingerprint store. Shared across install operations; mutated
    /// under `Mutex` and persisted by `install::install` on success.
    /// Consumed by #021's `handle_install`.
    pub tofu: Arc<Mutex<TofuStore>>,

    /// Installed-bundles registry. Loaded once per host startup; #021's
    /// `handle_install` clones under lock, passes `&mut clone` to
    /// `install::install`, and writes the result back on completion.
    pub bundles_registry: Arc<Mutex<RegistryHandle>>,

    /// Installed-themes registry. Populated at startup but not yet consumed
    /// by any dispatch handler — `handle_install` currently hard-codes
    /// `RegistryKind::Bundles`. Reserved for a future handler that branches
    /// on `artifact_kind` (theme vs bundle) to select the target registry.
    /// See spec #021 §2.1 and follow-up tracker.
    #[allow(dead_code)]
    pub themes_registry: Arc<Mutex<RegistryHandle>>,

    /// Server-fetched bundle size/entry limits. Cached at startup via
    /// `/v1/config/limits` with a periodic refresh; install requests read
    /// the current snapshot under `Mutex`. Consumed by #021's
    /// `handle_install`.
    pub limits: Arc<Mutex<BundleLimits>>,

    /// This host's running semver. `install::install` compares against the
    /// bundle's `omni_min_version` to surface `VersionMismatch` before any
    /// filesystem work runs. Consumed by #021's `handle_install`.
    pub current_version: Version,

    /// Single-slot live preview. `explorer.preview` writes, `explorer.cancelPreview`
    /// reads-and-cancels. See `share::preview::PreviewSlot` for lifecycle.
    /// Consumed by #021's `handle_preview` + `handle_cancel_preview`.
    pub preview_slot: Arc<PreviewSlot>,

    /// Per-request cancellation tokens keyed by WebSocket request `id`.
    /// #021's `handle_install` registers a fresh token on entry and removes
    /// it on any exit; the WS-disconnect cleanup path drains the registry
    /// so in-flight installs observe cancellation when the editor goes away.
    pub cancel_registry: Arc<Mutex<HashMap<String, CancellationToken>>>,

    /// Renderer-backed `ThemeSwap` implementation required by
    /// `PreviewSlot::start`. The host's overlay renderer owns the real impl;
    /// tests substitute a recording double. Consumed by #021's
    /// `handle_preview`.
    pub theme_swap: Arc<dyn ThemeSwap>,

    /// Root of the user's Omni workspace (contains `overlays/` and `themes/`).
    /// Relative `workspace_path` fields from `upload.pack` / `upload.publish`
    /// (e.g. `"overlays/Marathon"`) are resolved against this root. Renderer
    /// callers use the same relative format as `file.list` / `file.read`.
    pub data_dir: PathBuf,
}

impl ShareContext {
    /// Attempt to refresh `self.limits` from the Worker's
    /// `/v1/config/limits` endpoint. Called at the top of handlers that
    /// enforce limits (`handle_install`, `handle_publish`) so user-initiated
    /// share operations see fresh policy without waiting for a host restart.
    ///
    /// On network (or auth / server) failure, logs a warning and keeps the
    /// cached value — the host continues to function offline or through a
    /// transient Worker outage.
    ///
    /// Per architectural invariant #9a: the server owns evolving policy;
    /// clients enforce fresh-on-use rather than stale compile-time defaults.
    /// Security limits (invariant #9b) remain compile-time and are NOT
    /// affected by this path — only policy limits (bundle size / entry
    /// count) ride `BundleLimits`.
    ///
    /// Note: `ShareClient::config_limits` maintains its own 5-minute TTL
    /// cache, so back-to-back install/publish operations amortize to a
    /// single network round-trip.
    pub async fn try_refresh_limits(&self) {
        match self.client.config_limits().await {
            Ok(fresh) => {
                if let Ok(mut slot) = self.limits.lock() {
                    *slot = fresh;
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "limits refresh failed, using cached value"
                );
            }
        }
    }

    /// On-disk path for the persistent `identity-metadata.json` file
    /// (`backed_up`, `display_name`, rotation timestamps). Lives at
    /// `<data_dir>/identity-metadata.json`.
    ///
    /// Production: `data_dir` is `%APPDATA%\Omni\` (resolved by
    /// [`crate::config::data_dir`]) so this resolves to
    /// `%APPDATA%\Omni\identity-metadata.json` per spec §3.1.
    /// Tests: `test-harness::factories::build_share_context(tmp.path())`
    /// supplies a temp directory, so the metadata file is rooted there
    /// and never collides with a real user profile.
    ///
    /// Added by the 2026-04-26 identity-completion-and-display-name spec
    /// (Task 10) — the metadata path was previously read directly from
    /// `APPDATA` inside each handler, which broke under the test
    /// factory's `tempdir` data root. Routing through `data_dir` keeps
    /// production wiring identical (`config::data_dir() == APPDATA\Omni`)
    /// while giving tests proper isolation.
    pub fn identity_metadata_path(&self) -> PathBuf {
        self.data_dir.join("identity-metadata.json")
    }

    /// On-disk path for the active signing key file. Mirrors
    /// [`Self::identity_metadata_path`]'s rationale: production resolves
    /// to `%APPDATA%\Omni\identity.key` (matching the fallback at
    /// `crates/host/src/main.rs::resolve_identity_path`); tests get a
    /// `tempdir`-rooted path.
    ///
    /// Added by the 2026-04-26 identity-completion-and-display-name spec
    /// (Task 10) for use by `handle_identity_import` /
    /// `handle_identity_rotate` so the rotation/import primitives have a
    /// stable, test-isolatable target. Note: the production `OMNI_IDENTITY_PATH`
    /// env override applies only at host startup (`build_share_ctx`); the
    /// rotate/import path uses this method's `data_dir`-relative result so
    /// runtime rotations always land in the same directory the loader
    /// looks at on next startup.
    pub fn identity_key_path(&self) -> PathBuf {
        self.data_dir.join("identity.key")
    }
}

fn bad_input(id: &str, msg: impl Into<String>) -> String {
    error_envelope(
        id,
        &UploadError::BadInput {
            msg: msg.into(),
            source: None,
        },
    )
    .to_string()
}

/// Resolve a renderer-supplied `workspace_path` against the host's `data_dir`.
/// Absolute paths are returned unchanged so tests (and any future absolute-path
/// caller) keep working; relative paths — the renderer's format today — get
/// prefixed with `data_dir`.
fn resolve_workspace_path(data_dir: &std::path::Path, workspace_path: &str) -> PathBuf {
    let p = PathBuf::from(workspace_path);
    if p.is_absolute() {
        p
    } else {
        data_dir.join(p)
    }
}

fn parse_kind(s: Option<&str>) -> ArtifactKind {
    match s {
        Some("theme") => ArtifactKind::Theme,
        _ => ArtifactKind::Bundle,
    }
}

/// Top-level dispatch. Returns `Some(text)` to broadcast a synchronous result, or `None`
/// if the handler streams asynchronously via `send_fn`.
pub async fn dispatch<F>(ctx: &ShareContext, msg: &Value, send_fn: F) -> Option<String>
where
    F: Fn(String) + Send + Sync + Clone + 'static,
{
    let id = msg.get("id")?.as_str()?.to_string();
    let ty = msg.get("type")?.as_str()?;
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    match ty {
        "upload.pack" => {
            handle_pack(&id, params, ctx, send_fn).await;
            None
        }
        "upload.publish" => {
            handle_publish(&id, params, ctx, false, send_fn).await;
            None
        }
        "upload.update" => {
            handle_publish(&id, params, ctx, true, send_fn).await;
            None
        }
        "upload.delete" => handle_delete(&id, params, ctx).await,
        "identity.show" => handle_identity_show(&id, ctx).await,
        "identity.backup" => handle_identity_backup(&id, params, ctx).await,
        "identity.import" => handle_identity_import(&id, params, ctx).await,
        "identity.rotate" => handle_identity_rotate(&id, params, ctx).await,
        "identity.markBackedUp" => handle_identity_mark_backed_up(&id, params, ctx).await,
        "identity.setDisplayName" => handle_identity_set_display_name(&id, params, ctx).await,
        "config.vocab" => handle_config_vocab(&id, ctx).await,
        "config.limits" => handle_config_limits(&id, ctx).await,
        "report.submit" => handle_report(&id, params, ctx).await,
        "explorer.install" => handle_install(&id, params, ctx, send_fn).await,
        "explorer.preview" => handle_preview(&id, params, ctx).await,
        "explorer.cancelPreview" => handle_cancel_preview(&id, params, ctx).await,
        "explorer.list" => handle_list(&id, params, ctx).await,
        "explorer.get" => handle_get(&id, params, ctx).await,
        "workspace.listPublishables" => handle_list_publishables(&id, params, ctx).await,
        // Renderer-initiated single-image moderation gate (INV-7.7.2 site #1).
        // Fed by Step 2's Preview Image accept path; the renderer base64-
        // encodes the image bytes (matches the existing thumbnail-upload
        // pattern). The host singleton from `share::moderation` runs the
        // ONNX detector and returns the precomputed rejection flag.
        "share.moderationCheck" => handle_moderation_check(&id, params).await,
        _ => None,
    }
}

async fn handle_pack<F>(id: &str, params: Value, ctx: &ShareContext, send_fn: F)
where
    F: Fn(String) + Send + Sync + Clone + 'static,
{
    #[derive(Deserialize)]
    struct P {
        workspace_path: String,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        name: Option<String>,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => {
            send_fn(bad_input(id, format!("bad upload.pack params: {e}")));
            return;
        }
    };
    let req = UploadRequest {
        kind: parse_kind(p.kind.as_deref()),
        source_path: resolve_workspace_path(&ctx.data_dir, &p.workspace_path),
        name: p.name.unwrap_or_default(),
        description: String::new(),
        tags: Vec::<Tag>::new(),
        license: String::new(),
        version: Version::new(0, 0, 0),
        omni_min_version: Version::new(0, 0, 0),
        update_artifact_id: None,
    };
    // Propagate config_limits errors instead of silently defaulting — an auth
    // failure here must surface as SERVER_REJECT, not masquerade as a size error
    // once pack_only compares against the wrong limits. Network failures fall
    // back to the compile-time default (dry-run is a pre-flight; offline is
    // acceptable) with a logged warning.
    let limits = match ctx.client.config_limits().await {
        Ok(l) => l,
        Err(UploadError::Network(e)) => {
            tracing::warn!(
                error = %e,
                "config_limits network failure; falling back to BundleLimits::DEFAULT"
            );
            bundle::BundleLimits::DEFAULT
        }
        Err(e) => {
            send_fn(error_envelope(id, &e).to_string());
            return;
        }
    };
    // Per-stage `upload.packProgress` frames (spec §8.8 + INV-7.3.\*). The
    // pump task forwards each `PackProgress` from `pack_only_with_progress`'s
    // mpsc channel onto the WS as `{ id, type: "upload.packProgress", params: ... }`.
    // Channel size 16 is generous — only 5 stages × 2 transitions = 10 frames
    // typically, with headroom for future Wave B per-asset content-safety
    // sub-frames.
    let (pack_tx, mut pack_rx) = mpsc::channel::<PackProgress>(16);
    let id_for_pump = id.to_string();
    let send_for_pump = send_fn.clone();
    let pump = tokio::spawn(async move {
        while let Some(frame) = pack_rx.recv().await {
            let envelope = json!({
                "id": id_for_pump,
                "type": "upload.packProgress",
                "params": frame,
            });
            send_for_pump(envelope.to_string());
        }
    });

    // `pack_only_with_progress` takes `&Keypair`. Load once and pass the
    // deref-target through; the loaded `Guard<Arc<Keypair>>` keeps the
    // keypair alive even if a rotate swaps the slot mid-pack.
    let identity = ctx.identity.load_full();
    let result = pack_only_with_progress(&req, &limits, &identity, Some(&pack_tx)).await;
    // Drop the sender so the pump task's `recv()` returns None and the
    // task can exit. Without this drop the `pump.await` below would hang
    // indefinitely.
    drop(pack_tx);
    let _ = pump.await;

    match result {
        Ok(pack) => {
            let frame = json!({
                "id": id,
                "type": "upload.packResult",
                "params": {
                    "content_hash": pack.content_hash,
                    "compressed_size": pack.compressed_size,
                    "uncompressed_size": pack.uncompressed_size,
                    "manifest": pack.manifest,
                    "sanitize_report": pack.sanitize_report,
                }
            });
            send_fn(frame.to_string());
        }
        Err(e) => {
            send_fn(error_envelope(id, &e).to_string());
        }
    }
}

async fn handle_publish<F>(id: &str, params: Value, ctx: &ShareContext, is_update: bool, send_fn: F)
where
    F: Fn(String) + Send + Sync + Clone + 'static,
{
    // Full publish-params schema. Contract (ws-explorer.md §upload.publish) mandates
    // `workspace_path` + `visibility` + `bump`; sub-spec §2 requires the full manifest
    // metadata (name/description/tags/license/version/omni_min_version) to populate
    // `UploadRequest`. We accept both shapes: metadata fields ride alongside the
    // contract fields and are required for publish — without them the Worker's manifest
    // would be blank. Parse failures (bad semver, bad tag) return structured BadInput.
    #[derive(Deserialize)]
    struct P {
        workspace_path: String,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        artifact_id: Option<String>,
        #[serde(default)]
        #[allow(dead_code)]
        bump: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        tags: Option<Vec<String>>,
        #[serde(default)]
        license: Option<String>,
        #[serde(default)]
        version: Option<String>,
        #[serde(default)]
        omni_min_version: Option<String>,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => {
            send_fn(bad_input(id, format!("bad upload.publish params: {e}")));
            return;
        }
    };
    // Parse tags via the Tag::new gate so format errors surface as BadInput, not as
    // silent empty-string fallbacks. Vec<String> → Vec<Tag>.
    let tags: Vec<Tag> = match p
        .tags
        .unwrap_or_default()
        .into_iter()
        .map(Tag::new)
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(v) => v,
        Err(e) => {
            send_fn(bad_input(id, format!("invalid tag: {e}")));
            return;
        }
    };
    let version = match p.version.as_deref() {
        Some(s) => match Version::parse(s) {
            Ok(v) => v,
            Err(e) => {
                send_fn(bad_input(id, format!("invalid version {s:?}: {e}")));
                return;
            }
        },
        None => {
            send_fn(bad_input(id, "missing required field: version"));
            return;
        }
    };
    // `omni_min_version` is the minimum host version required to install
    // this artifact. The authoritative source is the running host's own
    // semver (ctx.current_version, sourced from CARGO_PKG_VERSION). If the
    // renderer omits the field — as the DEFAULT_FORM does for new publishes
    // — default to the current host version: "this was built and tested on
    // vX, so it should install on vX or later." Explicit user-supplied
    // values still win, in case the UI eventually exposes a picker.
    let omni_min_version = match p.omni_min_version.as_deref() {
        Some(s) => match Version::parse(s) {
            Ok(v) => v,
            Err(e) => {
                send_fn(bad_input(
                    id,
                    format!("invalid omni_min_version {s:?}: {e}"),
                ));
                return;
            }
        },
        None => ctx.current_version.clone(),
    };
    let req = UploadRequest {
        kind: parse_kind(p.kind.as_deref()),
        source_path: resolve_workspace_path(&ctx.data_dir, &p.workspace_path),
        name: p.name.unwrap_or_default(),
        description: p.description.unwrap_or_default(),
        tags,
        license: p.license.unwrap_or_default(),
        version,
        omni_min_version,
        update_artifact_id: if is_update { p.artifact_id } else { None },
    };

    // Refresh `ctx.limits` from the Worker before the pre-flight/upload
    // pipeline runs. Per invariant #9a, policy limits evolve server-side;
    // any consumer reading `ctx.limits` after this point (today: none in
    // the `upload()` path, which fetches fresh limits itself; tomorrow:
    // any future pre-check) sees the current snapshot.
    ctx.try_refresh_limits().await;

    let (tx, rx) = mpsc::channel(32);
    let id_cloned = id.to_string();
    let send_cloned = send_fn.clone();
    let pump = tokio::spawn(async move {
        let result_type = if is_update {
            "upload.updateResult"
        } else {
            "upload.publishResult"
        };
        pump_to_ws(&id_cloned, result_type, rx, send_cloned).await
    });
    // `upload` takes an owned `Arc<Keypair>` for its multi-stage path
    // (pack → sanitize → POST). `load_full` clones the inner Arc out of
    // the swap so the bundle signing key is captured at the start of
    // the upload. ShareClient.sign() — which signs the outgoing HTTP
    // request — reads the same shared swap on every call, so a rotate
    // mid-upload will desync the JWS kid from the bundle author and the
    // worker will reject. That's an accepted race: rotation is rare and
    // user-initiated; aborting in-flight uploads is the expected
    // semantics.
    let identity = ctx.identity.load_full();
    let res = upload(req, ctx.guard.clone(), identity, ctx.client.clone(), tx).await;
    let _ = pump.await;
    if let Err(e) = res {
        send_fn(error_envelope(id, &e).to_string());
    }
}

async fn handle_delete(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
    #[derive(Deserialize)]
    struct P {
        artifact_id: String,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad upload.delete params: {e}"))),
    };
    match ctx.client.delete(&p.artifact_id).await {
        Ok(()) => Some(
            json!({ "id": id, "type": "upload.deleteResult", "params": { "deleted": true } })
                .to_string(),
        ),
        Err(e) => Some(error_envelope(id, &e).to_string()),
    }
}

/// `identity.show` — return the full identity envelope per
/// `ws-explorer.md` §`identity.show` (extended by the 2026-04-26
/// identity-completion-and-display-name spec §5):
/// pubkey + fingerprint (hex/words/emoji) + `display_name` + `backed_up`
/// + `last_backed_up_at` + `last_rotated_at` + `last_backup_path`.
///
/// All persisted fields come from
/// [`IdentityMetadata::load_or_default`], which auto-tripwires on a
/// pubkey mismatch (e.g. user manually replaced `identity.key` out of
/// band) by resetting the metadata to defaults so a stale `backed_up:
/// true` never leaks into a different key's `identity.show` answer.
async fn handle_identity_show(id: &str, ctx: &ShareContext) -> Option<String> {
    let identity = ctx.identity.load();
    let pk = identity.public_key();
    let pk_hex = pk.to_hex();
    let fp = pk.fingerprint();

    let meta = IdentityMetadata::load_or_default(&ctx.identity_metadata_path(), &pk_hex);

    Some(
        json!({
            "id": id, "type": "identity.showResult",
            "params": {
                "pubkey_hex": pk_hex,
                "fingerprint_hex": fp.to_hex(),
                "fingerprint_words": fp.to_words(),
                "fingerprint_emoji": fp.to_emoji(),
                // Host doesn't yet track key-creation timestamps;
                // shipping `0` matches the prior behavior and the
                // contract permits the field as a Unix-seconds u64.
                "created_at": 0,
                "display_name": meta.display_name,
                "backed_up": meta.backed_up,
                "last_backed_up_at": meta.last_backed_up_at,
                "last_rotated_at": meta.last_rotated_at,
                "last_backup_path": meta.last_backup_path,
            }
        })
        .to_string(),
    )
}

async fn handle_identity_backup(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    #[derive(Deserialize)]
    struct P {
        passphrase: String,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad identity.backup params: {e}"))),
    };
    if p.passphrase.is_empty() {
        return Some(bad_input(id, "passphrase must not be empty"));
    }
    match ctx.identity.load().export_encrypted(&p.passphrase) {
        Ok(encrypted) => Some(
            json!({
                "id": id,
                "type": "identity.backupResult",
                "params": {
                    "encrypted_bytes_b64": STANDARD.encode(&encrypted),
                }
            })
            .to_string(),
        ),
        Err(e) => Some(bad_input(id, format!("identity.backup failed: {e}"))),
    }
}

/// `identity.import` — restore an Ed25519 keypair from an encrypted
/// `.omniid` backup blob, swap the active identity, and reset the
/// per-key `IdentityMetadata` so stale `backed_up`/`display_name` from
/// the prior key never carry across.
///
/// Params: `{ encrypted_bytes_b64, passphrase, overwrite_existing }`.
/// Result: `{ pubkey_hex, fingerprint_hex }` (matches existing contract
/// at `ws-explorer.md` §`identity.import`).
///
/// Overwrite-protection: when `overwrite_existing == false` AND the
/// on-disk `identity.key` already corresponds to a different pubkey,
/// returns a structured `error.code = "identity_already_exists"`
/// envelope so the editor can prompt the user before clobbering.
/// Re-importing the same key (matching pubkey) is always a no-op —
/// no error, no overwrite required.
///
/// On success: atomically writes the imported key to
/// [`ShareContext::identity_key_path`], `ctx.identity.store(...)` swaps
/// the active keypair (lock-free; in-flight signers observe the new
/// key on next `.load()`), metadata is reset to defaults seeded with
/// the new pubkey, and a best-effort `GET /v1/author/<new_pubkey>`
/// seeds `display_name` if the worker has one. Worker unreachable →
/// metadata stays with `display_name: None`; the next upload's
/// COALESCE upsert will catch the worker up if the user later sets a
/// name.
async fn handle_identity_import(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    #[derive(Deserialize)]
    struct P {
        encrypted_bytes_b64: String,
        passphrase: String,
        #[serde(default)]
        overwrite_existing: bool,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad identity.import params: {e}"))),
    };
    let bytes = match STANDARD.decode(&p.encrypted_bytes_b64) {
        Ok(b) => b,
        Err(e) => return Some(bad_input(id, format!("base64 decode failed: {e}"))),
    };

    // Decrypt-only first so we can validate the pubkey before deciding
    // whether to allow overwriting an existing on-disk identity. This
    // pre-check avoids a confusing "passphrase wrong but we wrote the
    // file anyway" flow.
    let decrypted = match Keypair::import_encrypted(&bytes, &p.passphrase) {
        Ok(k) => k,
        Err(e) => return Some(bad_input(id, format!("import failed: {e}"))),
    };
    let new_pk = decrypted.public_key();
    let new_pk_hex = new_pk.to_hex();
    let new_fp_hex = new_pk.fingerprint().to_hex();

    // Overwrite-protection: refuse if a different identity is already
    // on disk and the caller did not opt in. Re-importing the same key
    // is a no-op (we still rewrite the on-disk file as a self-heal in
    // case it was corrupted, but no error).
    let identity_path = ctx.identity_key_path();
    if !p.overwrite_existing {
        let active_pk_hex = ctx.identity.load().public_key().to_hex();
        if active_pk_hex != new_pk_hex {
            return Some(
                json!({
                    "id": id,
                    "type": "error",
                    "error": {
                        "code": "identity_already_exists",
                        "kind": "Admin",
                        "detail": null,
                        "message": "an identity already exists for this host; pass overwrite_existing=true to replace it",
                    }
                })
                .to_string(),
            );
        }
    }

    // Atomic disk swap. `import_encrypted_and_write` re-runs decrypt
    // (small Argon2 cost; the second pass keeps the identity-crate
    // public surface tight per architectural invariant #1) and writes
    // the OMNI-IDv1 envelope with user-only ACL. On failure the
    // existing file at `identity_path` is left untouched.
    let new_kp = match Keypair::import_encrypted_and_write(&bytes, &p.passphrase, &identity_path) {
        Ok(kp) => kp,
        Err(e) => return Some(bad_input(id, format!("import disk write failed: {e}"))),
    };

    // Reset metadata for the imported pubkey. Tripwire on
    // `load_or_default` would do this anyway on next read, but
    // persisting the reset now lets the editor immediately see clean
    // `backed_up: false` etc. without a second handler round-trip.
    let mut new_meta = IdentityMetadata {
        pubkey_hex: new_pk_hex.clone(),
        ..Default::default()
    };

    // Best-effort: seed `display_name` from the worker if it knows the
    // imported author. 404 → no display name yet, leave None;
    // network/transport failures → also leave None and let the user
    // re-set explicitly via `identity.setDisplayName`.
    if let Ok(detail) = ctx.client.get_author(&new_pk_hex).await {
        new_meta.display_name = detail.display_name;
    }
    if let Err(e) = IdentityMetadata::save(&ctx.identity_metadata_path(), &new_meta) {
        tracing::warn!(error = %e, "identity-metadata save failed after import");
    }

    // Swap the active keypair AFTER disk + metadata are durable. This
    // ordering matters: an in-flight signer that reads the slot
    // post-swap must see a key whose seed already lives at
    // `identity_path` so a host crash between swap and the next
    // request still recovers the correct identity on restart.
    ctx.identity.store(Arc::new(new_kp));

    Some(
        json!({
            "id": id,
            "type": "identity.importResult",
            "params": { "pubkey_hex": new_pk_hex, "fingerprint_hex": new_fp_hex }
        })
        .to_string(),
    )
}

/// `identity.rotate` — generate a fresh signing keypair, atomically
/// write it to `identity.key`, swap the active slot, carry the prior
/// `display_name` forward, and clear backup-state. Returns `{ pubkey_hex,
/// fingerprint_hex }` per `ws-explorer.md` §`identity.rotate`.
///
/// Spec §3.3 contract — `display_name` is **carried** (intent: same
/// human, new key); `backed_up` is **cleared** (intent: the user must
/// back up the new key before it's safe to publish), and
/// `last_rotated_at` is set so the editor can surface "you rotated
/// recently — confirm your backup". `last_backup_path` is also cleared
/// so a "show me my last backup" affordance never points at a file
/// that no longer corresponds to the active key.
///
/// The carried `display_name` is then fanned out best-effort via
/// `PUT /v1/author/me { display_name }` so the worker's authors table
/// pre-seeds for the new pubkey before the user's first publish under
/// the rotated identity. If the worker is unreachable the next upload
/// will COALESCE the same name; no synchronous failure on offline.
async fn handle_identity_rotate(id: &str, _params: Value, ctx: &ShareContext) -> Option<String> {
    let identity_path = ctx.identity_key_path();

    // Capture the prior display_name BEFORE swapping the keypair so we
    // read the metadata file under the current (pre-rotate) pubkey.
    let old_pk_hex = ctx.identity.load().public_key().to_hex();
    let old_meta = IdentityMetadata::load_or_default(&ctx.identity_metadata_path(), &old_pk_hex);
    let carried_display_name = old_meta.display_name.clone();

    let new_kp = match Keypair::generate_and_write(&identity_path) {
        Ok(kp) => kp,
        Err(e) => return Some(bad_input(id, format!("rotate failed: {e}"))),
    };
    let new_pk = new_kp.public_key();
    let new_pk_hex = new_pk.to_hex();
    let new_fp_hex = new_pk.fingerprint().to_hex();

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let new_meta = IdentityMetadata {
        pubkey_hex: new_pk_hex.clone(),
        display_name: carried_display_name.clone(),
        backed_up: false,
        last_backed_up_at: None,
        last_rotated_at: Some(now),
        last_backup_path: None,
    };
    if let Err(e) = IdentityMetadata::save(&ctx.identity_metadata_path(), &new_meta) {
        tracing::warn!(error = %e, "identity-metadata save failed after rotate");
    }

    // Swap the active keypair AFTER metadata persists (same ordering
    // as `handle_identity_import`).
    ctx.identity.store(Arc::new(new_kp));

    // Best-effort fan-out of the carried display_name. The PUT signs
    // with the **post-swap** keypair (the worker derives the target
    // pubkey from the JWS `kid`), so the row inserted on the worker
    // side is keyed to the new pubkey. Spawn detached so a slow worker
    // doesn't block the rotate result frame.
    if let Some(name) = carried_display_name {
        let client = ctx.client.clone();
        tokio::spawn(async move {
            if let Err(e) = client.set_display_name(&name).await {
                tracing::warn!(error = %e, "post-rotate set_display_name failed; will retry on next upload");
            }
        });
    }

    Some(
        json!({
            "id": id,
            "type": "identity.rotateResult",
            "params": { "pubkey_hex": new_pk_hex, "fingerprint_hex": new_fp_hex }
        })
        .to_string(),
    )
}

/// `identity.markBackedUp` — record that the user has saved an
/// encrypted backup of the active key to `path` at `timestamp` (Unix
/// seconds). Persists into [`IdentityMetadata`] without touching the
/// keypair.
///
/// Validation:
/// - `path` must be non-empty (worker doesn't see this; it's a local
///   hint for the editor's "open last backup" affordance)
/// - `timestamp` must be within ±86_400 seconds of the host's `now`
///   so a clock-skewed editor or a malicious caller can't backdate /
///   forward-date the backup record. Implementation symmetric: we
///   compute `|timestamp - now|` and reject when > 1 day.
///
/// Returns `{ ok: true }` on success, structured error envelope on
/// validation failure (mirrors how the editor surfaces other
/// validation rejections).
async fn handle_identity_mark_backed_up(
    id: &str,
    params: Value,
    ctx: &ShareContext,
) -> Option<String> {
    #[derive(Deserialize)]
    struct P {
        path: String,
        timestamp: u64,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad identity.markBackedUp params: {e}"))),
    };
    if p.path.is_empty() {
        return Some(bad_input(id, "path must not be empty"));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let drift = p.timestamp.abs_diff(now);
    if drift > 86_400 {
        return Some(bad_input(
            id,
            "timestamp must be within ±1 day of host time",
        ));
    }

    let pk_hex = ctx.identity.load().public_key().to_hex();
    let path = ctx.identity_metadata_path();
    let mut meta = IdentityMetadata::load_or_default(&path, &pk_hex);
    meta.backed_up = true;
    meta.last_backed_up_at = Some(p.timestamp);
    meta.last_backup_path = Some(p.path);
    if let Err(e) = IdentityMetadata::save(&path, &meta) {
        return Some(bad_input(id, format!("metadata persist failed: {e}")));
    }

    Some(
        json!({
            "id": id,
            "type": "identity.markBackedUpResult",
            "params": { "ok": true }
        })
        .to_string(),
    )
}

/// Validate a `display_name` candidate per spec §3.4 (pinned 2026-04-27):
/// 1. NFC-normalize via `unicode-normalization`
/// 2. Trim leading/trailing whitespace
/// 3. Length **in Unicode code points** (NFC scalar values), not UTF-16
///    code units, must be in `1..=32`. The worker counterpart measures
///    `[...normalized].length` which iterates JS code points; matching
///    via `chars().count()` keeps the boundary byte-for-byte aligned.
/// 4. Reject control characters (`\p{Cc}`)
/// 5. Reject surrogate code points (U+D800..U+DFFF) — Rust's `char` type
///    by construction cannot hold a surrogate, but we keep an explicit
///    range check so the failure-reason wording matches the worker for
///    the unrealistic case where a future input path can carry
///    surrogates as separate scalars.
///
/// Returns the trimmed, NFC-normalized name on success. The error is a
/// `&'static str` so callers can build the structured error envelope
/// with the exact wording from this function.
fn validate_display_name(raw: &str) -> Result<String, &'static str> {
    use unicode_normalization::UnicodeNormalization;
    let normalized: String = raw.nfc().collect();
    let trimmed = normalized.trim().to_string();
    let cp_count = trimmed.chars().count();
    if cp_count == 0 || cp_count > 32 {
        return Err("display_name must be 1-32 characters after trim");
    }
    for ch in trimmed.chars() {
        if ch.is_control() {
            return Err("display_name contains control characters");
        }
        let cp = ch as u32;
        if (0xD800..=0xDFFF).contains(&cp) {
            return Err("display_name contains surrogate code points");
        }
    }
    Ok(trimmed)
}

/// `identity.setDisplayName` — set or update the active author's
/// display name. Validates per [`validate_display_name`] (spec §3.4),
/// persists the normalized form to `IdentityMetadata`, and fires
/// `PUT /v1/author/me` best-effort. Returns
/// `{ display_name, pubkey_hex }` on success.
///
/// On validation failure: structured error envelope with
/// `code = "invalid_display_name"`, `kind = "Malformed"`, and the
/// specific reason in `message`.
///
/// Worker fan-out is non-blocking semantically (we wait for the
/// response so the user sees a synchronous success/failure), but a
/// worker error does NOT roll back the local persist — the spec §3.3
/// "the next upload's COALESCE will catch up" semantics apply.
async fn handle_identity_set_display_name(
    id: &str,
    params: Value,
    ctx: &ShareContext,
) -> Option<String> {
    #[derive(Deserialize)]
    struct P {
        display_name: String,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad identity.setDisplayName params: {e}"))),
    };
    let normalized = match validate_display_name(&p.display_name) {
        Ok(n) => n,
        Err(reason) => {
            return Some(
                json!({
                    "id": id,
                    "type": "error",
                    "error": {
                        "code": "invalid_display_name",
                        "kind": "Malformed",
                        "detail": null,
                        "message": reason,
                    }
                })
                .to_string(),
            )
        }
    };

    // Persist locally first so a worker failure doesn't desync the UI
    // — the editor can re-trigger `identity.show` and observe the new
    // name even if the worker round-trip was lost.
    let pk_hex = ctx.identity.load().public_key().to_hex();
    let path = ctx.identity_metadata_path();
    let mut meta = IdentityMetadata::load_or_default(&path, &pk_hex);
    meta.display_name = Some(normalized.clone());
    if let Err(e) = IdentityMetadata::save(&path, &meta) {
        tracing::warn!(error = %e, "identity-metadata save failed in setDisplayName");
    }

    // Fire to worker. Errors are logged but not surfaced — the user
    // already sees their name persisted locally; the next upload's
    // COALESCE upsert will sync to the worker if this round-trip
    // failed.
    if let Err(e) = ctx.client.set_display_name(&normalized).await {
        tracing::warn!(error = %e, "set_display_name worker call failed; local persist kept");
    }

    Some(
        json!({
            "id": id,
            "type": "identity.setDisplayNameResult",
            "params": { "display_name": normalized, "pubkey_hex": pk_hex }
        })
        .to_string(),
    )
}

async fn handle_config_vocab(id: &str, ctx: &ShareContext) -> Option<String> {
    match ctx.client.config_vocab().await {
        Ok(v) => Some(
            json!({
                "id": id, "type": "config.vocabResult",
                "params": { "tags": v.tags, "version": v.version }
            })
            .to_string(),
        ),
        Err(e) => Some(error_envelope(id, &e).to_string()),
    }
}

async fn handle_config_limits(id: &str, ctx: &ShareContext) -> Option<String> {
    match ctx.client.config_limits().await {
        Ok(l) => Some(
            json!({
                "id": id, "type": "config.limitsResult",
                "params": {
                    "max_bundle_compressed": l.max_bundle_compressed,
                    "max_bundle_uncompressed": l.max_bundle_uncompressed,
                    "max_entries": l.max_entries,
                    "version": 0, "updated_at": 0
                }
            })
            .to_string(),
        ),
        Err(e) => Some(error_envelope(id, &e).to_string()),
    }
}

async fn handle_report(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
    #[derive(Deserialize)]
    struct P {
        artifact_id: String,
        category: String,
        note: String,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad report.submit params: {e}"))),
    };
    match ctx
        .client
        .report(
            &p.artifact_id,
            ReportBody {
                category: p.category,
                note: p.note,
            },
        )
        .await
    {
        Ok(()) => Some(
            json!({
                "id": id, "type": "report.submitResult",
                "params": { "report_id": "", "status": "received" }
            })
            .to_string(),
        ),
        Err(e) => Some(error_envelope(id, &e).to_string()),
    }
}

/// Local adapter letting `handle_preview` pass an `Arc<dyn ThemeSwap>` into
/// `PreviewSlot::start`, whose `<S: ThemeSwap>` bound is `Sized`. The
/// newtype is itself `ThemeSwap` and forwards every call to the inner trait
/// object.
struct DynThemeSwap(Arc<dyn ThemeSwap>);

impl ThemeSwap for DynThemeSwap {
    fn snapshot(&self) -> Vec<u8> {
        self.0.snapshot()
    }
    fn apply(&self, css: &[u8]) -> Result<(), String> {
        self.0.apply(css)
    }
    fn revert(&self, snapshot: &[u8]) -> Result<(), String> {
        self.0.revert(snapshot)
    }
}

/// Dispatch arm for `explorer.install`.
///
/// Contract (per `specs/contracts/ws-explorer.md` §explorer.install):
/// - params: `{ artifact_id, target_workspace, overwrite?, expected_fingerprint_hex? }`
/// - progress frames: `explorer.installProgress { phase, done, total }` (zero or more)
/// - terminal: `explorer.installResult { installed_path, content_hash,
///   author_fingerprint_hex, tofu, warnings }` or `{ type: "error", error: ... }`
///
/// Lifecycle:
/// 1. Parse params → [`InstallRequest`].
/// 2. Register a fresh [`CancellationToken`] in `ctx.cancel_registry` keyed by `id`.
/// 3. Hold the `std::sync::Mutex` guards for `tofu`/`bundles_registry`/`limits`
///    across the nested `block_on(install::install(...))` call inside
///    [`tokio::task::block_in_place`]. This avoids the `MutexGuard<'_, T>: !Send`
///    constraint that would otherwise reject the guards being held across
///    `.await` inside the outer spawned future (see `dispatch_share_message`
///    in `ws_server.rs` — the future is spawned onto a multi-thread runtime
///    where `Send` is required). The brief blocking window is acceptable for
///    the single-user local host; simultaneous installs from one editor are
///    not expected and serializing them is correct behavior.
/// 4. Stream `InstallProgress` events through `send_fn` as they fire.
/// 5. Remove the cancel token from the registry on every exit path
///    (success / error / cancellation) via a scope-guarded wrapper.
///
/// `RegistryKind::Bundles` is hard-coded: install today targets the bundles
/// registry. A future extension may branch on params to pick themes.
///
/// # Runtime requirement
///
/// This handler (and the `dispatch` entry point that calls it) MUST run on
/// a `tokio::runtime::Runtime` built with `flavor = "multi_thread"`.
/// The implementation uses `tokio::task::block_in_place` to hold
/// `std::sync::MutexGuard`s across the inner `install::install` future's
/// `.await` points — a pattern that panics on a current-thread runtime.
/// The shipped `share_runtime` in `WsSharedState` is built with
/// `Builder::new_multi_thread()` (see `ws_server.rs`).
///
/// Integration-discovered fix (invariant #23): this dispatcher originally
/// read `expected_fingerprint_hex` and rejected any non-null value. #014
/// Wave 3a required end-to-end pubkey pinning for TOFU-mismatch coverage,
/// so the field name + parsing were brought in line with the shipped
/// `InstallRequest.expected_pubkey: Option<PublicKey>` in `install.rs` and
/// the `ws-explorer.md` §explorer.install contract (umbrella §4.7 retro
/// override). Original origin: #021 shipped the handler; #014 Wave 3a
/// surfaced the drift and fixed it inline.
async fn handle_install<F>(
    id: &str,
    params: Value,
    ctx: &ShareContext,
    send_fn: F,
) -> Option<String>
where
    F: Fn(String) + Send + Sync + Clone + 'static,
{
    #[derive(Deserialize)]
    struct P {
        artifact_id: String,
        #[serde(default)]
        target_workspace: Option<String>,
        #[serde(default)]
        overwrite: bool,
        #[serde(default)]
        expected_pubkey_hex: Option<String>,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad explorer.install params: {e}"))),
    };
    // Decode the optional pubkey pin. `PublicKey::from_hex` returns `None`
    // for any value that isn't exactly 64 lowercase hex characters (32 bytes).
    // A present-but-invalid hex string is rejected here with `BadInput` so
    // callers get a clear diagnostic rather than a silent fallback to unpinned.
    let expected_pubkey: Option<PublicKey> = match p.expected_pubkey_hex {
        None => None,
        Some(ref hex) => match PublicKey::from_hex(hex) {
            Some(pk) => Some(pk),
            None => {
                return Some(bad_input(
                    id,
                    "expected_pubkey_hex must be a 64-character lowercase hex Ed25519 pubkey",
                ))
            }
        },
    };
    // `target_workspace` is a contract-level string; we interpret a non-empty
    // value as a workspace-relative path and fall back to
    // `bundles/<artifact_id>` under the default workspace root. The install
    // pipeline takes an absolute `target_path`; callers without a prebaked
    // path must supply one or accept the default.
    let target_path: PathBuf = match p.target_workspace.as_deref() {
        Some(s) if !s.is_empty() => PathBuf::from(s),
        _ => PathBuf::from("bundles").join(&p.artifact_id),
    };

    let req = InstallRequest {
        artifact_id: p.artifact_id,
        target_path,
        overwrite: p.overwrite,
        expected_pubkey,
    };

    // Refresh `ctx.limits` from the Worker before enforcing. Per invariant
    // #9a, policy limits live server-side; the install pipeline reads
    // `ctx.limits` inside `block_in_place` below, so a fresh snapshot must
    // be landed in the mutex before that read.
    ctx.try_refresh_limits().await;

    // Register cancel token under request id. Scope-guard removal so every
    // exit path (Ok / Err / panic-free early-return) drains the entry.
    let cancel = CancellationToken::new();
    {
        let mut reg = ctx.cancel_registry.lock().unwrap();
        reg.insert(id.to_string(), cancel.clone());
    }
    let _cancel_cleanup = CancelRegistryGuard {
        registry: ctx.cancel_registry.clone(),
        id: id.to_string(),
    };

    // Progress streaming adapter — `install::install` invokes the closure
    // synchronously; we JSON-encode each frame and push via `send_fn`.
    let progress_send = {
        let send_fn = send_fn.clone();
        let id_owned = id.to_string();
        move |progress: InstallProgress| {
            let frame = install_progress_to_contract_frame(&id_owned, progress);
            send_fn(frame);
        }
    };

    // Hold the three Mutex guards across the nested install future. See
    // doc-comment above for the `block_in_place` rationale.
    let result = tokio::task::block_in_place(|| {
        let mut tofu_guard = ctx.tofu.lock().expect("tofu mutex poisoned");
        let mut registry_guard = ctx
            .bundles_registry
            .lock()
            .expect("bundles registry mutex poisoned");
        let limits = *ctx.limits.lock().expect("limits mutex poisoned");
        let version = ctx.current_version.clone();
        let handle = tokio::runtime::Handle::current();
        handle.block_on(install::install(
            req,
            &ctx.client,
            &mut tofu_guard,
            &mut registry_guard,
            RegistryKind::Bundles,
            &limits,
            &version,
            cancel,
            progress_send,
        ))
    });

    match result {
        Ok(outcome) => Some(install_outcome_to_result_frame(id, &outcome)),
        Err(e) => {
            let payload = handlers::map_install_error(&e);
            Some(handlers::error_frame(id, &payload))
        }
    }
}

/// Scope guard that removes an entry from `cancel_registry` on drop. Ensures
/// the registry doesn't leak tokens regardless of which exit path
/// `handle_install` takes.
struct CancelRegistryGuard {
    registry: Arc<Mutex<HashMap<String, CancellationToken>>>,
    id: String,
}

impl Drop for CancelRegistryGuard {
    fn drop(&mut self) {
        if let Ok(mut reg) = self.registry.lock() {
            reg.remove(&self.id);
        }
    }
}

/// Dispatch arm for `explorer.preview`.
///
/// Contract: params `{ artifact_id }` → result `{ preview_token }` on success,
/// D-004-J error envelope on failure.
///
/// The contract doesn't specify a TTL; we use a 60-second default, matching
/// the conservative lifetime implied by `PreviewSlot::start`'s tokio timer.
/// Preview CSS is fetched via `ctx.client.download(artifact_id, ...)` — for
/// theme artifacts the Worker serves raw CSS bytes from `/v1/download/:id`,
/// and `download()` returns those bytes directly. Installed-theme lookup
/// from the workspace is deferred; the simpler remote-fetch path covers the
/// current editor wire.
async fn handle_preview(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
    #[derive(Deserialize)]
    struct P {
        artifact_id: String,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad explorer.preview params: {e}"))),
    };

    // Fetch CSS bytes. download() is currently the only client surface that
    // returns raw artifact bytes; theme artifacts on the Worker are served
    // verbatim.
    let css = match ctx.client.download(&p.artifact_id, |_, _| {}).await {
        Ok(bytes) => bytes,
        Err(e) => {
            // Map download failure to a host-local payload; we do not have a
            // dedicated download-error mapper, so surface a compact envelope.
            let payload = ErrorPayload {
                code: "preview_download_failed",
                kind: "Io",
                detail: format!("download:{e}"),
                message: "Failed to fetch preview artifact.",
            };
            return Some(handlers::error_frame(id, &payload));
        }
    };

    const DEFAULT_PREVIEW_TTL: Duration = Duration::from_secs(60);
    // `PreviewSlot::start` takes `Arc<S: ThemeSwap>` — a sized generic —
    // while `ctx.theme_swap` is `Arc<dyn ThemeSwap>`. Wrap the trait object
    // in a local newtype that itself implements `ThemeSwap`, letting the
    // generic resolve cleanly without modifying `preview.rs`.
    let swap_adapter = Arc::new(DynThemeSwap(ctx.theme_swap.clone()));
    match ctx
        .preview_slot
        .start(swap_adapter, css, DEFAULT_PREVIEW_TTL)
    {
        Ok(token) => Some(
            json!({
                "id": id,
                "type": "explorer.previewResult",
                "preview_token": token.to_string(),
            })
            .to_string(),
        ),
        Err(e) => {
            let payload = map_preview_error(&e);
            Some(handlers::error_frame(id, &payload))
        }
    }
}

/// Dispatch arm for `explorer.cancelPreview`.
///
/// Contract: params `{ preview_token }` → result `{ restored: true }` on
/// success, D-004-J error envelope (`NO_ACTIVE_PREVIEW` / `TOKEN_MISMATCH`)
/// on failure. Parses `preview_token` as a UUID before calling
/// [`PreviewSlot::cancel`].
async fn handle_cancel_preview(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
    #[derive(Deserialize)]
    struct P {
        preview_token: String,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => {
            return Some(bad_input(
                id,
                format!("bad explorer.cancelPreview params: {e}"),
            ))
        }
    };
    let token = match Uuid::parse_str(&p.preview_token) {
        Ok(t) => t,
        Err(e) => {
            return Some(bad_input(
                id,
                format!("preview_token is not a valid UUID: {e}"),
            ));
        }
    };
    match ctx.preview_slot.cancel(token) {
        Ok(()) => Some(
            json!({
                "id": id,
                "type": "explorer.cancelPreviewResult",
                "restored": true,
            })
            .to_string(),
        ),
        Err(e) => {
            let payload = map_preview_error(&e);
            Some(handlers::error_frame(id, &payload))
        }
    }
}

/// Dispatch arm for `explorer.list`.
///
/// Contract (per `specs/contracts/ws-explorer.md` §explorer.list):
/// - params: `{ kind?, sort?, tags?, cursor?, limit? }`
/// - result: `explorer.listResult { items, next_cursor }`
///
/// Thin wrapper over [`ShareClient::list`]; `tags: Vec<String>` in the wire
/// shape maps to `tag: Vec<String>` in `ListParams`.
async fn handle_list(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
    #[derive(Deserialize)]
    struct P {
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        sort: Option<String>,
        #[serde(default)]
        tags: Option<Vec<String>>,
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        author_pubkey: Option<String>,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad explorer.list params: {e}"))),
    };
    let lp = ListParams {
        kind: p.kind,
        sort: p.sort,
        tag: p.tags.unwrap_or_default(),
        cursor: p.cursor,
        limit: p.limit,
        author_pubkey: p.author_pubkey,
    };
    match ctx.client.list(lp).await {
        Ok(lr) => {
            let items: Vec<Value> = match lr
                .items
                .iter()
                .map(serde_json::to_value)
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(v) => v,
                Err(e) => {
                    return Some(handlers::error_frame(
                        id,
                        &ErrorPayload {
                            code: "SERIALIZATION_ERROR",
                            kind: "HostLocal",
                            detail: format!("list_item_serialize_failed: {e}"),
                            message: "Worker returned an artifact we could not serialize.",
                        },
                    ));
                }
            };
            Some(
                json!({
                    "id": id,
                    "type": "explorer.listResult",
                    "items": items,
                    "next_cursor": lr.next_cursor,
                })
                .to_string(),
            )
        }
        Err(e) => Some(error_envelope(id, &e).to_string()),
    }
}

/// Dispatch arm for `explorer.get`.
///
/// Contract (per `specs/contracts/ws-explorer.md` §explorer.get):
/// - params: `{ artifact_id }`
/// - result: `explorer.getResult { artifact: <metadata> }`
///
/// The `<metadata>` shape is the full §4.4 wire shape mirrored by
/// [`super::client::ArtifactDetail`] — `manifest`, `reports`, `status`, etc.
/// Thin wrapper over [`ShareClient::get_artifact`]: error paths route
/// through `error_envelope` (parallel to `handle_list`) so Worker-side
/// `NOT_FOUND`/`TOMBSTONED`/`RATE_LIMITED` surface with their native codes
/// rather than being re-mapped into a host-local vocabulary.
async fn handle_get(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
    #[derive(Deserialize)]
    struct P {
        artifact_id: String,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => return Some(bad_input(id, format!("bad explorer.get params: {e}"))),
    };
    match ctx.client.get_artifact(&p.artifact_id).await {
        Ok(detail) => {
            let artifact = match serde_json::to_value(&detail) {
                Ok(v) => v,
                Err(e) => {
                    return Some(handlers::error_frame(
                        id,
                        &ErrorPayload {
                            code: "SERIALIZATION_ERROR",
                            kind: "HostLocal",
                            detail: format!("artifact_serialize_failed: {e}"),
                            message: "Worker returned an artifact we could not serialize.",
                        },
                    ));
                }
            };
            Some(
                json!({
                    "id": id,
                    "type": "explorer.getResult",
                    "artifact": artifact,
                })
                .to_string(),
            )
        }
        Err(e) => Some(error_envelope(id, &e).to_string()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// upload-flow-redesign Wave A0 — `upload.packProgress` wire contract
// ─────────────────────────────────────────────────────────────────────────────
//
// Spec: docs/superpowers/specs/2026-04-21-upload-flow-redesign-design.md §8.8.
//
// Renderer-side oracle: `apps/desktop/renderer/lib/share-types.ts`
// (`PackProgressSchema` + `ShareSubscriptionSchemas['upload.packProgress']`).
// Generated TypeScript view: `packages/shared-types/src/generated/PackProgress.ts`,
// `PackStage.ts`, `StageStatus.ts` (auto-emitted by `cargo test -p host`).
//
// The frame envelope sent over the wire is
// `{ id, type: "upload.packProgress", params: PackProgress }` — the `id`
// correlates with the originating `upload.pack` request and is constructed
// by the upload pipeline (Wave A1 task A1.4), not by this struct. `PackProgress`
// IS `params`.

/// Stage of the pack pipeline currently executing. Lower-case-kebab so the
/// JSON wire form matches the Zod enum
/// `'schema' | 'content-safety' | 'asset' | 'dependency' | 'size'` in
/// `share-types.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
#[serde(rename_all = "kebab-case")]
pub enum PackStage {
    Schema,
    ContentSafety,
    Asset,
    Dependency,
    Size,
}

/// Status of the current pack stage. Lower-case so the JSON wire form matches
/// `'running' | 'passed' | 'failed'` in `share-types.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
#[serde(rename_all = "lowercase")]
pub enum StageStatus {
    Running,
    Passed,
    Failed,
}

/// Per-stage progress payload streamed during `upload.pack`. `detail` carries
/// human-readable context (file name, violation summary) when relevant; on
/// `Failed` it carries the failure reason. The renderer accumulates one
/// `PackProgress` per stage into a `Record<PackStage, StageStatus>` for the
/// progressive Step 3 UI.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct PackProgress {
    pub stage: PackStage,
    pub status: StageStatus,
    pub detail: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// upload-flow-redesign Wave B1 — `share.moderationCheck` RPC
// ─────────────────────────────────────────────────────────────────────────────
//
// Spec: INV-7.7.2 site #1 (Step 2 Preview Image accept), INV-7.7.3 (threshold
// 0.8 — applied inside `share::moderation::check_image`), INV-7.7.4 (rejection
// copy + amber chrome owned renderer-side).
//
// Wire shape: renderer sends `{ image_base64: String }`; host base64-decodes,
// calls the process-global moderation singleton, and returns
// `{ unsafe_score, label, rejected }`. Mirrors the existing thumbnail-upload
// base64 transport pattern so renderer/main IPC stays text-only.
//
// Host startup wiring (OWI-73): `share::moderation::init_with_path` is called
// once from `run_host()` in `main.rs` before the WS server starts accepting
// connections, using the path resolved by `share::moderation::default_model_path()`.
// Degraded mode: if the bundled NSFW classifier model is missing (dev builds
// without the model staged), startup logs a warning and continues, and this
// handler returns a `Moderation:NotInitialized` error envelope per request.
// Renderer-side tests use a mock for `share.moderationCheck`; the production
// runtime path is live in any installer build that ships the bundled model.

/// Result frame payload emitted by `share.moderationCheck`. Mirrors
/// `share::moderation::CheckResult` 1-to-1 over the WS boundary; the
/// host wraps this in the standard `{ id, type, params }` envelope.
///
/// `unsafe_score` is the NSFW probability in `[0.0, 1.0]` — surfaced for
/// INV-7.7.6's collapsible detail block (`code Moderation:ClientRejected ·
/// detector onnx-falconsai-vit-v1 · confidence 0.XX`). `label` is `"nsfw"`
/// when the classifier leans NSFW, `"safe"` otherwise. `rejected` is the
/// precomputed `unsafe_score >= REJECTION_THRESHOLD (currently 0.5)` per
/// INV-7.7.3 — the renderer never reapplies the threshold.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct ModerationCheckResult {
    pub unsafe_score: f32,
    pub label: String,
    /// Detector ID(s) that produced the reported result. Populated by
    /// `share::moderation::CheckResult::detector` — single ID like
    /// `"onnx-nudenet-v1"` when one model fired (or both passed); joined
    /// `"a+b"` when both models rejected.
    pub detector: String,
    pub rejected: bool,
}

async fn handle_moderation_check(id: &str, params: Value) -> Option<String> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    #[derive(Deserialize)]
    struct P {
        image_base64: String,
    }

    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => {
            return Some(bad_input(
                id,
                format!("bad share.moderationCheck params: {e}"),
            ))
        }
    };

    let bytes = match STANDARD.decode(&p.image_base64) {
        Ok(b) => b,
        Err(e) => return Some(bad_input(id, format!("invalid base64 image_base64: {e}"))),
    };

    // `share::moderation::check_image` runs the inner ONNX inference under
    // the singleton mutex (per architectural invariant #24's documented
    // ownership in `share::moderation`). Done on the WS task — the model
    // load happens once at startup; per-call inference is bounded by the
    // 80–200 ms quantized ViT classifier latency. If the upload pipeline
    // ever batches multiple checks, the singleton's `Mutex` serializes them.
    match super::moderation::check_image(&bytes) {
        Ok(result) => {
            let payload = ModerationCheckResult {
                unsafe_score: result.unsafe_score,
                label: result.label,
                detector: result.detector,
                rejected: result.rejected,
            };
            Some(
                json!({
                    "id": id,
                    "type": "share.moderationCheckResult",
                    "params": payload,
                })
                .to_string(),
            )
        }
        Err(e) => {
            // Surface `NotInitialized` and `LockPoisoned` as Admin-kind so
            // the renderer treats them as host-config faults, not user-input
            // errors. Inner moderation errors (bad image bytes, unexpected
            // tensor shape) ride as Malformed.
            let (code, kind, message) = match &e {
                super::moderation::CheckError::NotInitialized => (
                    "Moderation:NotInitialized",
                    "Admin",
                    "Moderation model not initialized; host startup did not load the bundled detector",
                ),
                super::moderation::CheckError::LockPoisoned => (
                    "Moderation:LockPoisoned",
                    "Admin",
                    "Moderation model lock poisoned; host restart required",
                ),
                super::moderation::CheckError::Inner(_) => (
                    "Moderation:InvalidImage",
                    "Malformed",
                    "Moderation inference failed for the provided image bytes",
                ),
            };
            Some(
                json!({
                    "id": id,
                    "type": "error",
                    "error": {
                        "code": code,
                        "kind": kind,
                        "detail": e.to_string(),
                        "message": message,
                    }
                })
                .to_string(),
            )
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// upload-flow-redesign Wave A0 — `workspace.listPublishables` RPC
// ─────────────────────────────────────────────────────────────────────────────
//
// Spec: §8.8 + INV-7.1.10 (per-row metadata). Replaces the Step 1 source
// picker's previous `file.list` round-trip + per-item parallel reads with a
// single rich-row RPC: each entry carries the data needed to render the
// stack of overlay/theme rows (name, widget count, modified date, preview
// presence, sidecar update-mode signal) in one frame.

/// One row in the `workspace.listPublishables` response. Maps 1-to-1 with the
/// renderer's `PublishablesEntry` (zod schema in
/// `apps/desktop/renderer/lib/share-types.ts`).
///
/// Field semantics:
/// - `kind` — closed vocabulary `"overlay" | "theme"`. Kept as `String` for
///   forward-compat with future kinds (matches the convention used by
///   `CachedArtifactDetail.kind` and `PublishIndexEntry.kind`).
/// - `workspace_path` — relative path under `data_dir`, suitable for passing
///   straight back to `upload.pack` / `upload.publish` (e.g.
///   `"overlays/marathon-hud"` or `"themes/synth.css"`).
/// - `name` — display name (overlay folder name or theme filename without
///   extension).
/// - `widget_count` — `Some(n)` for overlays where `overlay.omni` parses;
///   `None` when the file is missing or unparseable, OR when the row is a
///   theme (themes have no widgets).
/// - `modified_at` — RFC 3339 / ISO-8601 string of the artifact's primary
///   file mtime (`overlay.omni` for overlays, the `.css` file for themes).
///   Empty string when mtime unavailable.
/// - `has_preview` — true when the corresponding `.omni-preview.png` (overlay)
///   or `<theme>.preview.png` (theme) exists. INV-7.1.9: when false the
///   renderer renders a zinc-gradient placeholder.
/// - `sidecar` — `Some(_)` when `.omni-publish.json` (overlays) or
///   `<theme>.css.publish.json` (themes) exists and parses. The renderer's
///   Step 1 banner (INV-7.1.13) and Step 4 update-mode pivot key off this
///   field's presence + the `author_pubkey_hex` match against the running
///   identity.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct PublishablesEntry {
    pub kind: String,
    pub workspace_path: String,
    pub name: String,
    pub widget_count: Option<u32>,
    pub modified_at: String,
    pub has_preview: bool,
    pub sidecar: Option<PublishSidecar>,
}

/// Convert a filesystem mtime to an RFC 3339 / ISO-8601 string. Returns the
/// empty string on any failure (clock skew before UNIX epoch, format error) —
/// the renderer's INV-7.1.10 metadata subtitle just hides the date when this
/// is empty rather than rendering "Modified <invalid>".
fn mtime_to_iso(modified: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    match modified.duration_since(UNIX_EPOCH) {
        Ok(d) => format_unix_secs_as_iso8601_utc(d.as_secs() as i64),
        Err(_) => String::new(),
    }
}

/// Format a UNIX timestamp (seconds) as `YYYY-MM-DDTHH:MM:SSZ`. Hand-rolled
/// to avoid pulling in `chrono` / `time` solely for this RPC. The renderer's
/// INV-7.1.10 only displays the date portion (`YYYY-MM-DD`); the time + Z
/// suffix are present for forward-compat with consumers that want full
/// resolution. Algorithm: Howard Hinnant's civil-from-days (public domain).
fn format_unix_secs_as_iso8601_utc(secs: i64) -> String {
    if secs < 0 {
        return String::new();
    }
    let secs_u = secs as u64;
    let day = (secs_u / 86_400) as i64;
    let time_of_day = secs_u % 86_400;
    let hour = (time_of_day / 3_600) as u32;
    let minute = ((time_of_day % 3_600) / 60) as u32;
    let second = (time_of_day % 60) as u32;

    // Civil-from-days: convert days since UNIX epoch (1970-01-01) to (y, m, d).
    let z = day + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, m, d, hour, minute, second
    )
}

/// Read mtime for `path`, returning the ISO-8601 string or empty on failure.
fn iso_mtime_of(path: &std::path::Path) -> String {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(mtime_to_iso)
        .unwrap_or_default()
}

/// Count widgets in an overlay's `overlay.omni`. Returns `None` if the file
/// doesn't exist or fails to parse. The renderer treats `None` as "show no
/// widget count" (INV-7.1.10 falls back to "Modified YYYY-MM-DD" alone).
fn overlay_widget_count(overlay_dir: &std::path::Path) -> Option<u32> {
    let omni_path = overlay_dir.join("overlay.omni");
    let source = std::fs::read_to_string(&omni_path).ok()?;
    let (parsed, _diagnostics) = parse_omni_with_diagnostics(&source);
    // We tolerate diagnostics — the source picker should still surface a
    // count for an overlay that emits warnings but successfully parses.
    parsed.map(|f| f.widgets.len() as u32)
}

async fn handle_list_publishables(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
    /// Optional `kind` filter ("overlay" | "theme"); omit / null returns both.
    /// Unknown values are treated as "no filter" rather than a hard error so a
    /// renderer that drifts ahead of the host still gets data instead of an
    /// empty list.
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct P {
        kind: Option<String>,
    }
    let p: P = match serde_json::from_value(params) {
        Ok(v) => v,
        Err(e) => {
            return Some(bad_input(
                id,
                format!("bad workspace.listPublishables params: {e}"),
            ))
        }
    };

    let kind_filter = p.kind.as_deref();
    let want_overlays = matches!(kind_filter, None | Some("overlay"));
    let want_themes = matches!(kind_filter, None | Some("theme"));

    let mut entries: Vec<PublishablesEntry> = Vec::new();

    if want_overlays {
        for name in list_overlays(&ctx.data_dir) {
            let overlay_dir = ctx.data_dir.join("overlays").join(&name);
            let omni_path = overlay_dir.join("overlay.omni");
            let preview_path = overlay_dir.join(".omni-preview.png");
            let sidecar = read_sidecar(&overlay_dir).ok().flatten();
            let modified_at = iso_mtime_of(&omni_path);
            let widget_count = overlay_widget_count(&overlay_dir);
            entries.push(PublishablesEntry {
                kind: "overlay".to_string(),
                workspace_path: format!("overlays/{}", name),
                name,
                widget_count,
                modified_at,
                has_preview: preview_path.exists(),
                sidecar,
            });
        }
    }

    if want_themes {
        let themes_dir = ctx.data_dir.join("themes");
        for filename in list_themes(&ctx.data_dir) {
            let css_path = themes_dir.join(filename.as_str());
            // Spec §8.3 / §7.1.9: theme preview lives at
            // `themes/<base>.preview.png`. Strip the `.css` extension to
            // derive the base, then suffix `.preview.png`. Falls back to
            // `<filename>.preview.png` when the strip would be a no-op.
            let preview_filename = match filename.strip_suffix(".css") {
                Some(base) => format!("{}.preview.png", base),
                None => format!("{}.preview.png", filename),
            };
            let preview_path = themes_dir.join(&preview_filename);
            let sidecar = read_theme_sidecar(&themes_dir, &filename).ok().flatten();
            let modified_at = iso_mtime_of(&css_path);
            // Display name: theme filename without `.css` extension. Matches
            // INV-7.1.10's "Modified YYYY-MM-DD" (themes have no widget count
            // line), and keeps `workspace_path` distinct (`themes/synth.css`)
            // for round-tripping back through `upload.pack`.
            let display_name = filename
                .strip_suffix(".css")
                .map(str::to_string)
                .unwrap_or_else(|| filename.clone());
            entries.push(PublishablesEntry {
                kind: "theme".to_string(),
                workspace_path: format!("themes/{}", filename),
                name: display_name,
                widget_count: None,
                modified_at,
                has_preview: preview_path.exists(),
                sidecar,
            });
        }
    }

    Some(
        json!({
            "id": id,
            "type": "workspace.listPublishablesResult",
            "params": {
                "entries": entries,
            },
        })
        .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::share::preview::ThemeSwap;
    use crate::share::registry::RegistryKind;
    use tempfile::TempDir;

    // Dispatch fan-out is covered by individual handler tests; here we verify
    // unknown `type` is ignored and malformed envelopes don't panic.

    /// Recording double for `ThemeSwap` used by tests that need a
    /// `ShareContext` but don't exercise preview.
    struct NoopSwap;
    impl ThemeSwap for NoopSwap {
        fn snapshot(&self) -> Vec<u8> {
            Vec::new()
        }
        fn apply(&self, _css: &[u8]) -> Result<(), String> {
            Ok(())
        }
        fn revert(&self, _snapshot: &[u8]) -> Result<(), String> {
            Ok(())
        }
    }

    /// Build a test-friendly `ShareContext` with the #021 fields defaulted
    /// to empty/stub values. The returned `TempDir` owns the tofu/registry
    /// backing files and must outlive the context.
    fn make_test_ctx() -> (ShareContext, TempDir) {
        let tmp = TempDir::new().expect("tempdir");
        // Wrap once in `Arc<ArcSwap<Keypair>>` so the test ctx and the
        // embedded ShareClient share the same swap slot — any future
        // rotate-style assertion that calls `ctx.identity.store(...)`
        // observes the change in `ctx.client` too.
        let kp = Arc::new(ArcSwap::new(Arc::new(Keypair::generate())));
        let guard: Arc<dyn Guard> = Arc::new(omni_guard_trait::StubGuard);
        let client = Arc::new(ShareClient::new(
            url::Url::parse("http://localhost:1/").unwrap(),
            kp.clone(),
            guard.clone(),
        ));
        let tofu = Arc::new(Mutex::new(TofuStore::open(tmp.path()).expect("tofu open")));
        let bundles_registry = Arc::new(Mutex::new(
            RegistryHandle::load(tmp.path(), RegistryKind::Bundles).expect("bundles registry"),
        ));
        let themes_registry = Arc::new(Mutex::new(
            RegistryHandle::load(tmp.path(), RegistryKind::Themes).expect("themes registry"),
        ));
        let limits = Arc::new(Mutex::new(BundleLimits::DEFAULT));
        let current_version = Version::new(0, 0, 0);
        let preview_slot = Arc::new(PreviewSlot::new());
        let cancel_registry = Arc::new(Mutex::new(HashMap::new()));
        let theme_swap: Arc<dyn ThemeSwap> = Arc::new(NoopSwap);
        let data_dir = tmp.path().to_path_buf();
        let ctx = ShareContext {
            identity: kp,
            guard,
            client,
            tofu,
            bundles_registry,
            themes_registry,
            limits,
            current_version,
            preview_slot,
            cancel_registry,
            theme_swap,
            data_dir,
        };
        (ctx, tmp)
    }

    #[tokio::test]
    async fn unknown_type_returns_none() {
        // Build a minimal context — no network calls will fire for an unknown type.
        // We construct ShareClient with a dummy URL; unknown-type dispatch returns before
        // touching the client.
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({ "id": "r1", "type": "no.such.type" });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await;
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn missing_id_returns_none() {
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({ "type": "upload.pack" });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await;
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn identity_show_returns_pubkey_hex() {
        let (ctx, _tmp) = make_test_ctx();
        let expected_pk = hex::encode(ctx.identity.load().public_key().0);
        let msg = json!({ "id": "r2", "type": "identity.show" });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "identity.showResult");
        assert_eq!(parsed["id"], "r2");
        assert_eq!(parsed["params"]["pubkey_hex"], expected_pk);
    }

    #[tokio::test]
    async fn identity_backup_returns_base64_encrypted_bytes() {
        // End-to-end check: the WS handler must call the identity crate's
        // `export_encrypted` and return a base64-encoded frame. If this regresses
        // (e.g. handler gets re-stubbed or the response shape drifts), the
        // renderer's identity-backup-dialog breaks silently during D3 smoke —
        // this test catches it at PR time instead of manual validation.
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;

        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({
            "id": "r-backup-1",
            "type": "identity.backup",
            "params": { "passphrase": "correct-horse-battery-staple" }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["id"], "r-backup-1");
        assert_eq!(parsed["type"], "identity.backupResult");
        let b64 = parsed["params"]["encrypted_bytes_b64"]
            .as_str()
            .expect("encrypted_bytes_b64 is a string");
        let decoded = STANDARD.decode(b64).expect("valid base64");
        assert!(!decoded.is_empty(), "backup bytes should be non-empty");
    }

    #[tokio::test]
    async fn identity_backup_rejects_empty_passphrase() {
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({
            "id": "r-backup-2",
            "type": "identity.backup",
            "params": { "passphrase": "" }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["code"], "BAD_INPUT");
    }

    #[tokio::test]
    async fn identity_backup_rejects_missing_passphrase() {
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({
            "id": "r-backup-3",
            "type": "identity.backup",
            "params": {}
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["code"], "BAD_INPUT");
    }

    #[tokio::test]
    async fn identity_show_reports_backed_up_false_until_006() {
        // backed_up drives the #015 first-publish gate. Wired as false until a
        // #006 follow-up persists real identity.backup events. If this ever
        // flips to a non-boolean or goes missing, the editor's Zod schema for
        // identity.show will reject the response — this test guards the
        // contract shape.
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({ "id": "r-backup-gate", "type": "identity.show" });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed["params"]["backed_up"],
            serde_json::Value::Bool(false)
        );
    }

    #[tokio::test]
    async fn upload_pack_bad_params_emits_error_envelope() {
        // Post OWI-40 (Task A1.6), `upload.pack` is a streaming handler:
        // packProgress frames + the terminal envelope all arrive via
        // `send_fn`, and `dispatch` returns `None`. Bad params still emit
        // a single error envelope (no progress frames precede it), so we
        // capture the broadcast and look for the BAD_INPUT envelope.
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({ "id": "r3", "type": "upload.pack", "params": { /* missing workspace_path */ } });
        let captured: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let captured_clone = captured.clone();
        let reply = dispatch(&ctx, &msg, move |s: String| {
            captured_clone.lock().unwrap().push(s);
        })
        .await;
        assert!(reply.is_none(), "upload.pack now streams via send_fn");
        let frames = captured.lock().unwrap().clone();
        let parsed: Value = serde_json::from_str(
            frames
                .iter()
                .find(|s| s.contains("\"type\":\"error\""))
                .expect("an error envelope must be sent"),
        )
        .unwrap();
        assert_eq!(parsed["error"]["code"], "BAD_INPUT");
    }

    #[test]
    fn resolve_workspace_path_joins_relative_against_data_dir() {
        // Regression: bug hit 2026-04-18 — handle_pack / handle_publish fed
        // `"overlays/Marathon"` straight into `UploadRequest.source_path`,
        // leaving it relative to the host's CWD instead of `data_dir`. Every
        // real upload failed with os error 3 "path not found".
        let data_dir = std::path::Path::new("C:\\Users\\test\\AppData\\Roaming\\omni");
        let resolved = super::resolve_workspace_path(data_dir, "overlays/Marathon");
        assert_eq!(
            resolved,
            data_dir.join("overlays").join("Marathon"),
            "relative workspace_path must be joined against data_dir"
        );
    }

    #[test]
    fn resolve_workspace_path_passes_absolute_through_unchanged() {
        // Absolute paths (tests, future callers) must not be re-rooted under
        // `data_dir` — that would turn an absolute path into a nonsense
        // nested path on Unix and silently corrupt it on Windows.
        #[cfg(windows)]
        let abs = "C:\\tmp\\my-theme";
        #[cfg(not(windows))]
        let abs = "/tmp/my-theme";
        let data_dir = std::path::Path::new("/unused");
        let resolved = super::resolve_workspace_path(data_dir, abs);
        assert_eq!(resolved, std::path::PathBuf::from(abs));
    }

    #[test]
    fn share_context_fields_are_constructible() {
        // Pins the #021 field set: if a field is added/removed/renamed,
        // `make_test_ctx` construction either breaks here or in downstream
        // consumers, forcing a coordinated update.
        let (ctx, _tmp) = make_test_ctx();
        assert!(ctx.cancel_registry.lock().unwrap().is_empty());
        assert!(!ctx.preview_slot.is_active());
        assert_eq!(ctx.current_version, Version::new(0, 0, 0));
    }

    // ---- #021 handler tests --------------------------------------------

    /// `handle_install` must drain the cancel registry on every exit path.
    /// Here the install pipeline fails fast because the configured client
    /// points at `http://localhost:1/` (no listener) — the download step
    /// surfaces a `DownloadError::Http`, which maps to an
    /// `InstallError::IoFailure` error frame. Regardless of that mapping,
    /// the scope-guarded `CancelRegistryGuard` must remove the request's
    /// entry before `handle_install` returns.
    ///
    /// `multi_thread` flavor is required because `handle_install` invokes
    /// `tokio::task::block_in_place`, which panics on the single-thread
    /// runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_install_registers_cancel_then_removes_on_error() {
        let (ctx, _tmp) = make_test_ctx();
        let id = "install-err";
        let msg = json!({
            "id": id,
            "type": "explorer.install",
            "params": { "artifact_id": "abc", "target_workspace": "" }
        });
        // Pre-condition: registry is empty before dispatch.
        assert!(ctx.cancel_registry.lock().unwrap().is_empty());
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        // The reply must be an error envelope (download fails fast).
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            parsed["type"], "error",
            "expected error frame, got {parsed:?}"
        );
        // Post-condition: cancel registry has been drained even though
        // install returned Err — the scope guard fires on every exit path.
        assert!(
            ctx.cancel_registry.lock().unwrap().is_empty(),
            "cancel_registry not drained after error exit"
        );
    }

    /// A BadInput exit (malformed params) must NOT register a cancel token
    /// since the install pipeline never starts. Complements the error-path
    /// test above: together they pin that (a) registrations only happen
    /// once params parse, and (b) registrations that DO happen always get
    /// cleaned up.
    #[tokio::test]
    async fn handle_install_bad_input_does_not_register_cancel() {
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({
            "id": "install-bad",
            "type": "explorer.install",
            "params": { /* missing artifact_id */ }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["code"], "BAD_INPUT");
        assert!(ctx.cancel_registry.lock().unwrap().is_empty());
    }

    // ---- INV23 regression tests (Wave 3a #014) ---------------------------------

    /// Positive case: `explorer.install` with a valid `expected_pubkey_hex`
    /// (64-hex, deterministic zero key) must NOT return `BAD_INPUT`. The
    /// install pipeline proceeds past param-parsing and fails at the network
    /// download step (ctx client points at `http://localhost:1/`), confirming
    /// the pubkey was accepted and `InstallRequest.expected_pubkey` was
    /// populated. If the dispatcher had rejected the non-null field (the
    /// pre-INV23 behaviour) this would surface as `BAD_INPUT` instead.
    ///
    /// `multi_thread` flavor required because `handle_install` uses
    /// `tokio::task::block_in_place`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_install_expected_pubkey_hex_accepted_and_forwarded() {
        let (ctx, _tmp) = make_test_ctx();
        // Deterministic 32-byte zero key encoded as 64 lowercase hex chars.
        let zero_pubkey_hex = "00".repeat(32);
        let msg = json!({
            "id": "inv23-pos",
            "type": "explorer.install",
            "params": {
                "artifact_id": "some-artifact",
                "expected_pubkey_hex": zero_pubkey_hex,
            }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        // Must NOT be BAD_INPUT — the pubkey parsed successfully and the
        // pipeline was entered. The error is a network failure (no listener
        // at localhost:1), not a param rejection.
        assert_ne!(
            parsed["error"]["code"], "BAD_INPUT",
            "expected_pubkey_hex was rejected as BAD_INPUT (pre-INV23 regression): {parsed:?}"
        );
        assert_eq!(
            parsed["type"], "error",
            "expected a non-BAD_INPUT error (network failure), got: {parsed:?}"
        );
    }

    /// Negative case: a request carrying the OLD field name
    /// `expected_fingerprint_hex` (the pre-INV23 contract drift) must NOT
    /// cause the dispatcher to read or act on the value. After the rename,
    /// serde ignores unknown fields by default, so the field is silently
    /// dropped. The install proceeds normally (reaching the network-failure
    /// error), confirming the rename is effective — not additive — and the
    /// old name is no longer wired to any logic.
    ///
    /// `multi_thread` flavor required because `handle_install` uses
    /// `tokio::task::block_in_place`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_install_old_fingerprint_hex_field_is_ignored() {
        let (ctx, _tmp) = make_test_ctx();
        let zero_pubkey_hex = "00".repeat(32);
        let msg = json!({
            "id": "inv23-neg",
            "type": "explorer.install",
            "params": {
                "artifact_id": "some-artifact",
                // OLD field name — must be silently ignored after the rename.
                "expected_fingerprint_hex": zero_pubkey_hex,
            }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        // The old field name is unknown to serde and is silently dropped.
        // The dispatcher must NOT reject it with BAD_INPUT. The install
        // continues without pubkey pinning and fails at the network step.
        assert_ne!(
            parsed["error"]["code"], "BAD_INPUT",
            "old field name caused BAD_INPUT — rename may not have taken effect: {parsed:?}"
        );
        assert_eq!(
            parsed["type"], "error",
            "expected a network error (not BAD_INPUT), got: {parsed:?}"
        );
    }

    /// `handle_preview` returns `PREVIEW_ACTIVE` when the slot already
    /// holds a session. Pre-start one via `PreviewSlot::start` directly
    /// (no network) to occupy the slot, then drive the handler and assert
    /// the error code.
    ///
    /// Note: the handler calls `ctx.client.download()` BEFORE consulting
    /// the slot — we arrange for download to succeed (wiremock) so the
    /// slot check is the failure point. Without wiremock the client would
    /// surface `preview_download_failed` first, which would not exercise
    /// the preview-active branch.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_preview_slot_occupied_returns_preview_active() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/download/theme-1"))
            .respond_with(
                ResponseTemplate::new(200).set_body_bytes(b"body { color: red; }".to_vec()),
            )
            .mount(&server)
            .await;

        let tmp = TempDir::new().unwrap();
        let kp = Arc::new(ArcSwap::new(Arc::new(Keypair::generate())));
        let guard: Arc<dyn Guard> = Arc::new(omni_guard_trait::StubGuard);
        let client = Arc::new(ShareClient::new(
            url::Url::parse(&format!("{}/", server.uri())).unwrap(),
            kp.clone(),
            guard.clone(),
        ));
        let tofu = Arc::new(Mutex::new(TofuStore::open(tmp.path()).unwrap()));
        let bundles_registry = Arc::new(Mutex::new(
            RegistryHandle::load(tmp.path(), RegistryKind::Bundles).unwrap(),
        ));
        let themes_registry = Arc::new(Mutex::new(
            RegistryHandle::load(tmp.path(), RegistryKind::Themes).unwrap(),
        ));
        let preview_slot = Arc::new(PreviewSlot::new());
        let theme_swap: Arc<dyn ThemeSwap> = Arc::new(NoopSwap);
        let data_dir = tmp.path().to_path_buf();
        let ctx = ShareContext {
            identity: kp,
            guard,
            client,
            tofu,
            bundles_registry,
            themes_registry,
            limits: Arc::new(Mutex::new(BundleLimits::DEFAULT)),
            current_version: Version::new(0, 0, 0),
            preview_slot: preview_slot.clone(),
            cancel_registry: Arc::new(Mutex::new(HashMap::new())),
            theme_swap: theme_swap.clone(),
            data_dir,
        };

        // Pre-occupy the slot with an unrelated session so the handler's
        // own call surfaces `PreviewActive`.
        let adapter = Arc::new(DynThemeSwap(theme_swap));
        preview_slot
            .start(adapter, b"occupied".to_vec(), Duration::from_secs(60))
            .expect("pre-start");

        let msg = json!({
            "id": "prev-1",
            "type": "explorer.preview",
            "params": { "artifact_id": "theme-1" }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["code"], "PREVIEW_ACTIVE");
        assert_eq!(parsed["error"]["kind"], "HostLocal");
    }

    /// `handle_cancel_preview` returns `NO_ACTIVE_PREVIEW` when the slot
    /// is empty, regardless of the token value.
    #[tokio::test]
    async fn handle_cancel_preview_unknown_token_returns_no_active_preview() {
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({
            "id": "cancel-1",
            "type": "explorer.cancelPreview",
            "params": { "preview_token": Uuid::new_v4().to_string() }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["code"], "NO_ACTIVE_PREVIEW");
    }

    /// `handle_cancel_preview` with a malformed `preview_token` (not a
    /// UUID) returns a BadInput envelope — parse errors short-circuit
    /// before the slot is consulted.
    #[tokio::test]
    async fn handle_cancel_preview_malformed_token_returns_bad_input() {
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({
            "id": "cancel-bad",
            "type": "explorer.cancelPreview",
            "params": { "preview_token": "not-a-uuid" }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["code"], "BAD_INPUT");
    }

    /// `handle_get` happy path: wiremock serves a §4.4-shaped artifact body,
    /// the handler round-trips it as an `explorer.getResult` frame carrying
    /// the full metadata under `artifact`. Replaces the pre-follow-up
    /// `NOT_IMPLEMENTED` stub test.
    #[tokio::test]
    async fn handle_get_returns_artifact_on_success() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = json!({
            "artifact_id": "art-get",
            "kind": "bundle",
            "manifest": { "name": "demo", "version": "1.0.0" },
            "content_hash": "deadbeef".repeat(8),
            "r2_url": "https://r2.example/bundle",
            "thumbnail_url": "https://r2.example/thumb.png",
            "author_pubkey": "aa".repeat(32),
            "author_fingerprint_hex": "aa11bb22cc33",
            "installs": 7,
            "reports": 0,
            "created_at": 1_700_000_000_i64,
            "updated_at": 1_700_000_000_i64,
            "status": "live",
        });
        Mock::given(method("GET"))
            .and(path("/v1/artifact/art-get"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
            .mount(&server)
            .await;

        let base = format!("{}/", server.uri());
        let (ctx, _tmp) = make_test_ctx_with_base(&base);
        let msg = json!({
            "id": "get-ok",
            "type": "explorer.get",
            "params": { "artifact_id": "art-get" }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "explorer.getResult");
        assert_eq!(parsed["id"], "get-ok");
        let artifact = &parsed["artifact"];
        assert_eq!(artifact["artifact_id"], "art-get");
        assert_eq!(artifact["kind"], "bundle");
        assert_eq!(artifact["status"], "live");
        assert_eq!(artifact["installs"], 7);
        assert_eq!(artifact["reports"], 0);
        assert_eq!(artifact["author_fingerprint_hex"], "aa11bb22cc33");
        // `manifest` is forwarded as a subtree per worker-api §4.4.
        assert_eq!(artifact["manifest"]["name"], "demo");
    }

    /// `handle_get` error path: wiremock returns a 404 with a worker-shaped
    /// error body; handler must surface the Worker's `NOT_FOUND` code
    /// verbatim through `error_envelope` (not remapped into a host-local
    /// vocabulary). Mirrors `handle_list`'s error handling.
    #[tokio::test]
    async fn handle_get_maps_not_found_to_error_envelope() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let err_body = json!({
            "error": { "code": "NOT_FOUND", "message": "no such artifact" }
        });
        Mock::given(method("GET"))
            .and(path("/v1/artifact/missing"))
            .respond_with(ResponseTemplate::new(404).set_body_json(err_body))
            .mount(&server)
            .await;

        let base = format!("{}/", server.uri());
        let (ctx, _tmp) = make_test_ctx_with_base(&base);
        let msg = json!({
            "id": "get-404",
            "type": "explorer.get",
            "params": { "artifact_id": "missing" }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["id"], "get-404");
        // Worker-returned code surfaces as `SERVER_REJECT` per `UploadError::code`,
        // with the §3 code riding in `detail` / `message`. `error_envelope` is the
        // same path `handle_list` exercises; pinning the envelope shape here
        // prevents drift between the two handlers.
        assert_eq!(parsed["error"]["code"], "SERVER_REJECT");
        assert_eq!(parsed["error"]["kind"], "Malformed");
    }

    /// Malformed params (missing `artifact_id`) short-circuit to a
    /// `BAD_INPUT` envelope before the client is consulted.
    #[tokio::test]
    async fn handle_get_bad_params_returns_bad_input() {
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({
            "id": "get-bad",
            "type": "explorer.get",
            "params": { /* missing artifact_id */ }
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["code"], "BAD_INPUT");
    }

    // ---- `try_refresh_limits` tests (phase-2 followup #4) --------------

    /// Build a `ShareContext` whose `client` points at the supplied base
    /// URL and whose `limits` mutex starts at `BundleLimits::DEFAULT`.
    /// Mirrors `make_test_ctx` but lets the caller inject a wiremock base.
    fn make_test_ctx_with_base(base: &str) -> (ShareContext, TempDir) {
        let tmp = TempDir::new().expect("tempdir");
        let kp = Arc::new(ArcSwap::new(Arc::new(Keypair::generate())));
        let guard: Arc<dyn Guard> = Arc::new(omni_guard_trait::StubGuard);
        let client = Arc::new(ShareClient::new(
            url::Url::parse(base).expect("base url parse"),
            kp.clone(),
            guard.clone(),
        ));
        let tofu = Arc::new(Mutex::new(TofuStore::open(tmp.path()).expect("tofu open")));
        let bundles_registry = Arc::new(Mutex::new(
            RegistryHandle::load(tmp.path(), RegistryKind::Bundles).expect("bundles registry"),
        ));
        let themes_registry = Arc::new(Mutex::new(
            RegistryHandle::load(tmp.path(), RegistryKind::Themes).expect("themes registry"),
        ));
        let limits = Arc::new(Mutex::new(BundleLimits::DEFAULT));
        let current_version = Version::new(0, 0, 0);
        let preview_slot = Arc::new(PreviewSlot::new());
        let cancel_registry = Arc::new(Mutex::new(HashMap::new()));
        let theme_swap: Arc<dyn ThemeSwap> = Arc::new(NoopSwap);
        let data_dir = tmp.path().to_path_buf();
        let ctx = ShareContext {
            identity: kp,
            guard,
            client,
            tofu,
            bundles_registry,
            themes_registry,
            limits,
            current_version,
            preview_slot,
            cancel_registry,
            theme_swap,
            data_dir,
        };
        (ctx, tmp)
    }

    /// On success, `try_refresh_limits` replaces the mutex contents with
    /// the Worker's fresh snapshot. Wiremock returns a limits payload whose
    /// values are distinct from `BundleLimits::DEFAULT` so the assertion
    /// can distinguish "cached" from "fresh."
    #[tokio::test]
    async fn try_refresh_limits_success_updates_mutex() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        // Distinct values from BundleLimits::DEFAULT (5 MiB / 10 MiB / 32).
        let fresh = json!({
            "max_bundle_compressed": 7u64 * 1024 * 1024,
            "max_bundle_uncompressed": 14u64 * 1024 * 1024,
            "max_entries": 64usize,
        });
        Mock::given(method("GET"))
            .and(path("/v1/config/limits"))
            .respond_with(ResponseTemplate::new(200).set_body_json(fresh))
            .mount(&server)
            .await;

        let base = format!("{}/", server.uri());
        let (ctx, _tmp) = make_test_ctx_with_base(&base);

        // Pre-condition: mutex holds the compile-time default.
        {
            let slot = ctx.limits.lock().expect("limits mutex");
            assert_eq!(
                slot.max_bundle_compressed,
                BundleLimits::DEFAULT.max_bundle_compressed
            );
            assert_eq!(
                slot.max_bundle_uncompressed,
                BundleLimits::DEFAULT.max_bundle_uncompressed
            );
            assert_eq!(slot.max_entries, BundleLimits::DEFAULT.max_entries);
        }

        ctx.try_refresh_limits().await;

        // Post-condition: mutex now holds the Worker's snapshot.
        let slot = ctx.limits.lock().expect("limits mutex");
        assert_eq!(slot.max_bundle_compressed, 7 * 1024 * 1024);
        assert_eq!(slot.max_bundle_uncompressed, 14 * 1024 * 1024);
        assert_eq!(slot.max_entries, 64);
    }

    /// On network / server failure, `try_refresh_limits` logs a warning
    /// and leaves the cached value in place — host continues to function
    /// offline or through transient Worker outages (invariant #9a intent:
    /// fresh-on-use, but fail-open for availability).
    #[tokio::test]
    async fn try_refresh_limits_failure_keeps_cached() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        // 404 is non-transient — it surfaces directly as `ServerReject`
        // without walking the backoff budget. `try_refresh_limits` must
        // absorb any `Err` variant and keep the cached value; testing with
        // 404 (not 500) keeps this test fast and deterministic while
        // exercising the same error path.
        Mock::given(method("GET"))
            .and(path("/v1/config/limits"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;

        let base = format!("{}/", server.uri());
        let (ctx, _tmp) = make_test_ctx_with_base(&base);

        // Pre-condition: DEFAULT.
        {
            let slot = ctx.limits.lock().expect("limits mutex");
            assert_eq!(
                slot.max_bundle_compressed,
                BundleLimits::DEFAULT.max_bundle_compressed
            );
            assert_eq!(
                slot.max_bundle_uncompressed,
                BundleLimits::DEFAULT.max_bundle_uncompressed
            );
            assert_eq!(slot.max_entries, BundleLimits::DEFAULT.max_entries);
        }

        ctx.try_refresh_limits().await;

        // Post-condition: unchanged (cached value preserved on failure).
        let slot = ctx.limits.lock().expect("limits mutex");
        assert_eq!(
            slot.max_bundle_compressed,
            BundleLimits::DEFAULT.max_bundle_compressed
        );
        assert_eq!(
            slot.max_bundle_uncompressed,
            BundleLimits::DEFAULT.max_bundle_uncompressed
        );
        assert_eq!(slot.max_entries, BundleLimits::DEFAULT.max_entries);
    }

    // ── upload-flow-redesign Wave A0 — workspace.listPublishables ───────

    /// Stage a workspace with one parseable overlay (with `.omni-publish.json`
    /// sidecar + `.omni-preview.png`) and one theme. The handler must return
    /// both rows with the right kind / paths / counts / preview flags / sidecar.
    #[tokio::test]
    async fn list_publishables_returns_overlay_and_theme_rows() {
        use crate::share::sidecar::{write_sidecar, write_theme_sidecar, PublishSidecar};

        let (ctx, _tmp) = make_test_ctx();

        // Stage an overlay with the minimal parseable .omni file. Uses the
        // documented overlay.omni format so the parser can count widgets.
        let overlay_dir = ctx.data_dir.join("overlays").join("marathon-hud");
        std::fs::create_dir_all(&overlay_dir).unwrap();
        let omni_source = "[widget.cpu]\ntemplate = <div>cpu</div>\n";
        std::fs::write(overlay_dir.join("overlay.omni"), omni_source).unwrap();
        // Drop a fake .omni-preview.png so has_preview flips to true.
        std::fs::write(overlay_dir.join(".omni-preview.png"), b"fake png").unwrap();
        // Sidecar — the renderer keys the linked-artifact banner off this.
        // The description/tags/license fields were added in OWI-110 to support
        // INV-7.5.3 Step 2 prefill; this test fixture leaves them empty since
        // it asserts the linked-artifact banner contract, not prefill.
        let sidecar = PublishSidecar {
            artifact_id: "ov_test".to_string(),
            author_pubkey_hex: hex::encode(ctx.identity.load().public_key().0),
            version: "1.0.0".to_string(),
            last_published_at: "2026-04-21T00:00:00Z".to_string(),
            description: String::new(),
            tags: Vec::new(),
            license: String::new(),
        };
        write_sidecar(&overlay_dir, &sidecar).unwrap();

        // Stage a theme + theme sidecar.
        let themes_dir = ctx.data_dir.join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();
        std::fs::write(themes_dir.join("synth.css"), b"body { color: cyan; }").unwrap();
        write_theme_sidecar(&themes_dir, "synth.css", &sidecar).unwrap();

        let msg = json!({
            "id": "lp-1",
            "type": "workspace.listPublishables",
            "params": {},
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["id"], "lp-1");
        assert_eq!(parsed["type"], "workspace.listPublishablesResult");
        let entries = parsed["params"]["entries"].as_array().expect("entries");
        // Overlay row first (handler enumerates overlays before themes).
        let overlay = &entries[0];
        assert_eq!(overlay["kind"], "overlay");
        assert_eq!(overlay["name"], "marathon-hud");
        assert_eq!(overlay["workspace_path"], "overlays/marathon-hud");
        assert_eq!(overlay["has_preview"], serde_json::Value::Bool(true));
        // Sidecar surfaces the original struct verbatim.
        assert_eq!(overlay["sidecar"]["artifact_id"], "ov_test");
        // Theme row.
        let theme = entries
            .iter()
            .find(|e| e["kind"] == "theme")
            .expect("theme row");
        assert_eq!(theme["name"], "synth");
        assert_eq!(theme["workspace_path"], "themes/synth.css");
        assert_eq!(theme["widget_count"], serde_json::Value::Null);
        assert_eq!(theme["sidecar"]["artifact_id"], "ov_test");
    }

    /// `kind: "overlay"` filter narrows to overlay rows; themes are dropped.
    #[tokio::test]
    async fn list_publishables_kind_filter_narrows_results() {
        let (ctx, _tmp) = make_test_ctx();

        let overlay_dir = ctx.data_dir.join("overlays").join("hud");
        std::fs::create_dir_all(&overlay_dir).unwrap();
        std::fs::write(overlay_dir.join("overlay.omni"), b"").unwrap();

        let themes_dir = ctx.data_dir.join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();
        std::fs::write(themes_dir.join("dark.css"), b"").unwrap();

        let msg = json!({
            "id": "lp-2",
            "type": "workspace.listPublishables",
            "params": { "kind": "overlay" },
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let entries = parsed["params"]["entries"].as_array().expect("entries");
        assert!(entries.iter().all(|e| e["kind"] == "overlay"));
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["name"], "hud");
    }

    /// Empty workspace produces an empty entries array (no panic, no error).
    #[tokio::test]
    async fn list_publishables_empty_workspace_returns_empty() {
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({
            "id": "lp-3",
            "type": "workspace.listPublishables",
            "params": {},
        });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        let entries = parsed["params"]["entries"].as_array().expect("entries");
        assert!(entries.is_empty());
    }

    /// PackProgress / PackStage / StageStatus serialize to the kebab/lowercase
    /// strings the renderer Zod schema expects. Locks the contract shape so a
    /// future serde rename_all change can't silently break the wire.
    #[test]
    fn pack_progress_wire_shape_matches_renderer_schema() {
        let pp = PackProgress {
            stage: PackStage::ContentSafety,
            status: StageStatus::Failed,
            detail: Some("nudity score 0.87".to_string()),
        };
        let v = serde_json::to_value(&pp).unwrap();
        assert_eq!(v["stage"], "content-safety");
        assert_eq!(v["status"], "failed");
        assert_eq!(v["detail"], "nudity score 0.87");

        // Stages render as kebab-case for everything that needs a separator.
        assert_eq!(serde_json::to_value(PackStage::Schema).unwrap(), "schema");
        assert_eq!(
            serde_json::to_value(PackStage::Dependency).unwrap(),
            "dependency"
        );
        // Statuses are flat lowercase.
        assert_eq!(
            serde_json::to_value(StageStatus::Running).unwrap(),
            "running"
        );
        assert_eq!(serde_json::to_value(StageStatus::Passed).unwrap(), "passed");
    }
}
