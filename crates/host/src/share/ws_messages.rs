//! WebSocket message handlers for `upload.*` / `identity.*` / `config.*` / `report.submit`.
//! Wire shapes are authoritative in ws-explorer.md — do not invent fields here.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use omni_bundle::{BundleLimits, Tag};
use omni_guard_trait::Guard;
use omni_identity::{Keypair, PublicKey};
use semver::Version;
use serde::Deserialize;
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
use super::install::{self, InstallProgress, InstallRequest};
use super::preview::{PreviewSlot, ThemeSwap};
use super::progress::{error_envelope, pump_to_ws};
use super::registry::{RegistryHandle, RegistryKind};
use super::tofu::TofuStore;
use super::upload::{upload, ArtifactKind, UploadRequest};

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
    pub identity: Arc<Keypair>,
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
        "upload.pack" => handle_pack(&id, params, ctx).await,
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
        "config.vocab" => handle_config_vocab(&id, ctx).await,
        "config.limits" => handle_config_limits(&id, ctx).await,
        "report.submit" => handle_report(&id, params, ctx).await,
        "explorer.install" => handle_install(&id, params, ctx, send_fn).await,
        "explorer.preview" => handle_preview(&id, params, ctx).await,
        "explorer.cancelPreview" => handle_cancel_preview(&id, params, ctx).await,
        "explorer.list" => handle_list(&id, params, ctx).await,
        "explorer.get" => handle_get(&id, params, ctx).await,
        _ => None,
    }
}

async fn handle_pack(id: &str, params: Value, ctx: &ShareContext) -> Option<String> {
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
        Err(e) => return Some(bad_input(id, format!("bad upload.pack params: {e}"))),
    };
    let req = UploadRequest {
        kind: parse_kind(p.kind.as_deref()),
        source_path: p.workspace_path.into(),
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
            omni_bundle::BundleLimits::DEFAULT
        }
        Err(e) => return Some(error_envelope(id, &e).to_string()),
    };
    match super::upload::pack_only(&req, &limits, &ctx.identity).await {
        Ok(pack) => Some(
            json!({
                "id": id,
                "type": "upload.packResult",
                "params": {
                    "content_hash": pack.content_hash,
                    "compressed_size": pack.compressed_size,
                    "uncompressed_size": pack.uncompressed_size,
                    "manifest": pack.manifest,
                    "sanitize_report": pack.sanitize_report,
                }
            })
            .to_string(),
        ),
        Err(e) => Some(error_envelope(id, &e).to_string()),
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
        None => {
            send_fn(bad_input(id, "missing required field: omni_min_version"));
            return;
        }
    };
    let req = UploadRequest {
        kind: parse_kind(p.kind.as_deref()),
        source_path: p.workspace_path.into(),
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
    let res = upload(
        req,
        ctx.guard.clone(),
        ctx.identity.clone(),
        ctx.client.clone(),
        tx,
    )
    .await;
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

async fn handle_identity_show(id: &str, ctx: &ShareContext) -> Option<String> {
    let pk_bytes = ctx.identity.public_key().0;
    let pk_hex = hex::encode(pk_bytes);
    // Full fingerprint rendering (hex/words/emoji) is owned by sub-spec #006's
    // surface. Until that API surfaces here, return the pubkey + empty-field
    // envelope so the editor can render "not yet implemented" without crashing.
    Some(
        json!({
            "id": id, "type": "identity.showResult",
            "params": {
                "pubkey_hex": pk_hex,
                "fingerprint_hex": "",
                "fingerprint_words": Vec::<String>::new(),
                "fingerprint_emoji": Vec::<String>::new(),
                "created_at": 0
            }
        })
        .to_string(),
    )
}

/// Identity management (backup/import/rotate) is owned by a sub-spec #006
/// follow-up. Until then the WS surface returns a structured error envelope
/// instead of canned empty payloads.
fn identity_not_implemented(id: &str) -> Option<String> {
    Some(
        json!({
            "id": id,
            "type": "error",
            "error": {
                "code": "NOT_IMPLEMENTED",
                "kind": "Admin",
                "detail": null,
                "message": "Identity management handled by sub-spec #006 follow-up",
            }
        })
        .to_string(),
    )
}

async fn handle_identity_backup(id: &str, _params: Value, _ctx: &ShareContext) -> Option<String> {
    identity_not_implemented(id)
}

async fn handle_identity_import(id: &str, _params: Value, _ctx: &ShareContext) -> Option<String> {
    identity_not_implemented(id)
}

async fn handle_identity_rotate(id: &str, _params: Value, _ctx: &ShareContext) -> Option<String> {
    identity_not_implemented(id)
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
async fn handle_install<F>(id: &str, params: Value, ctx: &ShareContext, send_fn: F) -> Option<String>
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
        let kp = Arc::new(Keypair::generate());
        let guard: Arc<dyn Guard> = Arc::new(omni_guard_trait::StubGuard);
        let client = Arc::new(ShareClient::new(
            url::Url::parse("http://localhost:1/").unwrap(),
            kp.clone(),
            guard.clone(),
        ));
        let tofu = Arc::new(Mutex::new(
            TofuStore::open(tmp.path()).expect("tofu open"),
        ));
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
        let expected_pk = hex::encode(ctx.identity.public_key().0);
        let msg = json!({ "id": "r2", "type": "identity.show" });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "identity.showResult");
        assert_eq!(parsed["id"], "r2");
        assert_eq!(parsed["params"]["pubkey_hex"], expected_pk);
    }

    #[tokio::test]
    async fn upload_pack_bad_params_emits_error_envelope() {
        let (ctx, _tmp) = make_test_ctx();
        let msg = json!({ "id": "r3", "type": "upload.pack", "params": { /* missing workspace_path */ } });
        let out = dispatch(&ctx, &msg, |_s: String| {}).await.expect("reply");
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["code"], "BAD_INPUT");
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
        assert_eq!(parsed["type"], "error", "expected error frame, got {parsed:?}");
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
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"body { color: red; }".to_vec()))
            .mount(&server)
            .await;

        let tmp = TempDir::new().unwrap();
        let kp = Arc::new(Keypair::generate());
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
        let kp = Arc::new(Keypair::generate());
        let guard: Arc<dyn Guard> = Arc::new(omni_guard_trait::StubGuard);
        let client = Arc::new(ShareClient::new(
            url::Url::parse(base).expect("base url parse"),
            kp.clone(),
            guard.clone(),
        ));
        let tofu = Arc::new(Mutex::new(
            TofuStore::open(tmp.path()).expect("tofu open"),
        ));
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
            assert_eq!(slot.max_bundle_compressed, BundleLimits::DEFAULT.max_bundle_compressed);
            assert_eq!(slot.max_bundle_uncompressed, BundleLimits::DEFAULT.max_bundle_uncompressed);
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
            assert_eq!(slot.max_bundle_compressed, BundleLimits::DEFAULT.max_bundle_compressed);
            assert_eq!(slot.max_bundle_uncompressed, BundleLimits::DEFAULT.max_bundle_uncompressed);
            assert_eq!(slot.max_entries, BundleLimits::DEFAULT.max_entries);
        }

        ctx.try_refresh_limits().await;

        // Post-condition: unchanged (cached value preserved on failure).
        let slot = ctx.limits.lock().expect("limits mutex");
        assert_eq!(slot.max_bundle_compressed, BundleLimits::DEFAULT.max_bundle_compressed);
        assert_eq!(slot.max_bundle_uncompressed, BundleLimits::DEFAULT.max_bundle_uncompressed);
        assert_eq!(slot.max_entries, BundleLimits::DEFAULT.max_entries);
    }
}
