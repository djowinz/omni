//! WebSocket message handlers for `upload.*` / `identity.*` / `config.*` / `report.submit`.
//! Wire shapes are authoritative in ws-explorer.md — do not invent fields here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use omni_bundle::{BundleLimits, Tag};
use omni_guard_trait::Guard;
use omni_identity::Keypair;
use semver::Version;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::client::{ReportBody, ShareClient};
use super::error::UploadError;
use super::preview::{PreviewSlot, ThemeSwap};
use super::progress::{error_envelope, pump_to_ws};
use super::registry::RegistryHandle;
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

    /// Installed-themes registry. Same lifecycle pattern as
    /// `bundles_registry`; kept as a sibling field so the dispatcher can pick
    /// which registry a given `explorer.install` targets by artifact kind.
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
}
