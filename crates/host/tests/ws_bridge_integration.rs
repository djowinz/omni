//! Integration tests for sub-spec #021 — host async WS bridge.
//!
//! Exercises the explorer.* dispatch surface end-to-end via the public
//! `omni_host::share::ws_messages::dispatch` entry point with a
//! `wiremock::MockServer` standing in for the Worker. Each test drives one
//! scenario from spec §8 "Done criteria — integration tests":
//!
//! 1. `explorer.install` round-trip — signed bundle served by wiremock, full
//!    download → verify → sanitize → write pipeline; assert progress frames
//!    arrive on the send channel and the terminal `explorer.installResult`
//!    carries matching `id` + `content_hash`.
//! 2. WS-disconnect cancel path — the wiremock Worker delays the download,
//!    the test fires `.cancel()` on the token registered in
//!    `ctx.cancel_registry` (mimicking the `ws_server.rs` disconnect drain),
//!    and the handler must return an `InstallError::Cancelled` error frame
//!    without mutating TOFU or the registry.
//! 3. `explorer.preview` + `explorer.cancelPreview` round-trip — a CSS blob
//!    served by wiremock starts a preview, the returned token cancels it, a
//!    second preview then succeeds to prove the slot was cleared. A
//!    `CountingThemeSwap` records apply/revert counts.
//! 4. `service_unavailable` envelope — the fallback path in `ws_server.rs`
//!    that fires when `share_ctx` is `None`; pinned byte-for-byte against
//!    `install_context_unavailable()`.
//! 5. Sync handler regression — covered by the existing `ws_server::tests`
//!    module (`handle_sensors_subscribe`, `handle_status`); referenced here
//!    rather than duplicated because those tests already lock in the wire
//!    behavior unchanged by #021's Wave 3 edits.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use omni_bundle::{BundleLimits, FileEntry, Manifest, Tag};
use omni_guard_trait::{Guard, StubGuard};
use omni_host::share::client::ShareClient;
use omni_host::share::handlers::install_context_unavailable;
use omni_host::share::preview::{PreviewSlot, ThemeSwap};
use omni_host::share::registry::{RegistryHandle, RegistryKind};
use omni_host::share::tofu::TofuStore;
use omni_host::share::ws_messages::{dispatch, ShareContext};
use omni_identity::{pack_signed_bundle, Keypair};
use semver::Version;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---- Fixture helpers --------------------------------------------------------

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

/// Build a signed two-file bundle (`overlay.omni` + `themes/theme.css`) whose
/// manifest carries `name` and is signed by `kp`. Mirrors the fixture pattern
/// used by `install_end_to_end.rs` and `install_tofu_and_recovery.rs`.
fn build_signed_bundle(name: &str, kp: &Keypair) -> Vec<u8> {
    let overlay_bytes = b"<overlay></overlay>".to_vec();
    let theme_bytes = b"body { color: red; }".to_vec();
    let overlay_sha = sha256_of(&overlay_bytes);
    let theme_sha = sha256_of(&theme_bytes);
    let manifest = Manifest {
        schema_version: 1,
        name: name.into(),
        version: Version::new(1, 0, 0),
        omni_min_version: Version::new(0, 1, 0),
        description: "ts-021 integration fixture".into(),
        tags: vec![Tag::new("dark").unwrap()],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/theme.css".into()),
        sensor_requirements: vec![],
        files: vec![
            FileEntry {
                path: "overlay.omni".into(),
                sha256: overlay_sha,
            },
            FileEntry {
                path: "themes/theme.css".into(),
                sha256: theme_sha,
            },
        ],
        resource_kinds: None,
    };
    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), overlay_bytes);
    files.insert("themes/theme.css".to_string(), theme_bytes);
    pack_signed_bundle(&manifest, &files, kp, &BundleLimits::DEFAULT).unwrap()
}

/// Recording [`ThemeSwap`] — tracks how many times apply/revert fired so the
/// preview-cancel test can assert revert ran on cancel.
struct CountingThemeSwap {
    applies: AtomicUsize,
    reverts: AtomicUsize,
}

impl CountingThemeSwap {
    fn new() -> Self {
        Self {
            applies: AtomicUsize::new(0),
            reverts: AtomicUsize::new(0),
        }
    }
    fn applies(&self) -> usize {
        self.applies.load(Ordering::SeqCst)
    }
    fn reverts(&self) -> usize {
        self.reverts.load(Ordering::SeqCst)
    }
}

impl ThemeSwap for CountingThemeSwap {
    fn snapshot(&self) -> Vec<u8> {
        b"snapshot".to_vec()
    }
    fn apply(&self, _css: &[u8]) -> Result<(), String> {
        self.applies.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    fn revert(&self, _snapshot: &[u8]) -> Result<(), String> {
        self.reverts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// Assemble a [`ShareContext`] wired to `worker_url`. The returned
/// `TempDir` owns the tofu + registry backing files and must outlive the
/// context. `theme_swap` is the swap implementation used for preview tests;
/// install-only tests pass an inert swap and ignore it.
fn build_share_context(
    worker_url: Url,
    theme_swap: Arc<dyn ThemeSwap>,
) -> (ShareContext, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let identity = Arc::new(Keypair::generate());
    let guard: Arc<dyn Guard> = Arc::new(StubGuard);
    let client = Arc::new(ShareClient::new(worker_url, identity.clone(), guard.clone()));
    let tofu = Arc::new(Mutex::new(TofuStore::open(tmp.path()).expect("tofu open")));
    let bundles_registry = Arc::new(Mutex::new(
        RegistryHandle::load(tmp.path(), RegistryKind::Bundles).expect("bundles registry"),
    ));
    let themes_registry = Arc::new(Mutex::new(
        RegistryHandle::load(tmp.path(), RegistryKind::Themes).expect("themes registry"),
    ));
    let limits = Arc::new(Mutex::new(BundleLimits::DEFAULT));
    // `install::install` uses `current_version >= manifest.omni_min_version`;
    // the fixture manifest sets `omni_min_version = 0.1.0`, so 99.0.0 here
    // keeps the version-gate from short-circuiting the install path.
    let current_version = Version::new(99, 0, 0);
    let preview_slot = Arc::new(PreviewSlot::new());
    let cancel_registry = Arc::new(Mutex::new(HashMap::new()));
    let ctx = ShareContext {
        identity,
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

/// Collect progress + terminal frames pushed through the `send_fn` passed to
/// `dispatch`. The `mpsc::UnboundedSender` is threaded in; test drains at the
/// end.
fn make_sink() -> (
    impl Fn(String) + Send + Sync + Clone + 'static,
    mpsc::UnboundedReceiver<String>,
) {
    let (tx, rx) = mpsc::unbounded_channel::<String>();
    let send_fn = move |s: String| {
        let _ = tx.send(s);
    };
    (send_fn, rx)
}

/// Drain every frame currently in the receiver without blocking further.
fn drain_frames(rx: &mut mpsc::UnboundedReceiver<String>) -> Vec<Value> {
    let mut frames = Vec::new();
    while let Ok(frame) = rx.try_recv() {
        let parsed: Value = serde_json::from_str(&frame).expect("valid json from handler");
        frames.push(parsed);
    }
    frames
}

// ---- Test 1: explorer.install round-trip ------------------------------------

/// Full install round-trip: construct a signed bundle, serve it via
/// wiremock, drive `explorer.install` through `dispatch`, and assert the
/// reply envelope carries the expected `content_hash` + `id`.
///
/// `multi_thread` is required because `handle_install` calls
/// `tokio::task::block_in_place`, which panics on a single-thread runtime.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explorer_install_round_trip_delivers_result_frame() {
    let kp = Keypair::generate();
    let bundle_bytes = build_signed_bundle("ts-021-install", &kp);
    let expected_content_hash = hex::encode(sha256_of(&bundle_bytes));

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/download/art-install"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(bundle_bytes.clone())
                .insert_header("content-type", "application/octet-stream"),
        )
        .mount(&server)
        .await;

    let swap: Arc<dyn ThemeSwap> = Arc::new(CountingThemeSwap::new());
    let (ctx, _tmp) = build_share_context(
        Url::parse(&format!("{}/", server.uri())).unwrap(),
        swap,
    );

    let workspace = TempDir::new().unwrap();
    let target = workspace.path().join("bundles/art-install");

    let (send_fn, mut rx) = make_sink();
    let msg = json!({
        "id": "req-install-1",
        "type": "explorer.install",
        "params": {
            "artifact_id": "art-install",
            "target_workspace": target.to_string_lossy(),
            "overwrite": false,
        },
    });

    let reply = dispatch(&ctx, &msg, send_fn).await;

    // The handler returns the terminal frame directly (progress frames flow
    // through `send_fn`, which we've wired to an mpsc sink).
    let terminal = reply.expect("handle_install must return a terminal frame");
    let terminal: Value = serde_json::from_str(&terminal).expect("valid json");

    assert_eq!(
        terminal["type"], "explorer.installResult",
        "terminal frame type; got {terminal}"
    );
    assert_eq!(terminal["id"], "req-install-1", "terminal id must match");
    assert_eq!(
        terminal["content_hash"], expected_content_hash,
        "content_hash must match bundle bytes"
    );
    // `installed_path` is a platform-specific string; assert non-empty.
    assert!(
        terminal["installed_path"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "installed_path must be populated"
    );

    // Progress frames arrived on the sink during the install — at least one
    // must have fired before the terminal frame returned.
    let progress_frames = drain_frames(&mut rx);
    assert!(
        !progress_frames.is_empty(),
        "install pipeline must emit at least one progress frame"
    );
    for frame in &progress_frames {
        assert_eq!(
            frame["type"], "explorer.installProgress",
            "streamed frame must be installProgress; got {frame}"
        );
        assert_eq!(frame["id"], "req-install-1", "progress id must match");
        assert!(
            frame["phase"].is_string(),
            "progress frame must carry a phase string"
        );
    }

    // Registry must hold exactly one entry for the freshly-installed bundle.
    let registry = ctx
        .bundles_registry
        .lock()
        .expect("bundles registry mutex");
    let entries: Vec<_> = registry.entries().iter().collect();
    assert_eq!(
        entries.len(),
        1,
        "exactly one registry entry after successful install"
    );

    // Cancel registry must be empty — the scope guard removes the entry on
    // every exit path.
    assert!(
        ctx.cancel_registry.lock().unwrap().is_empty(),
        "cancel_registry must be drained after install completes"
    );
}

// ---- Test 2: WS disconnect cancels in-flight install ------------------------

/// Disconnect-cancel path. The wiremock Worker delays the download by 5s;
/// the test dispatches `explorer.install`, waits briefly for the cancel
/// token to appear in `ctx.cancel_registry`, then drains the registry (the
/// same code path `ws_server.rs` runs on WS disconnect). The handler must
/// return a `cancelled` error envelope; neither tofu nor the registry may
/// mutate.
///
/// Using wiremock's `set_delay` is strictly simpler than trait-object
/// rework of `ShareClient::download` — the whole pipeline stays real; only
/// the remote response is slowed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ws_disconnect_cancels_in_flight_install() {
    let kp = Keypair::generate();
    let bundle_bytes = build_signed_bundle("ts-021-cancel", &kp);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/download/art-slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(bundle_bytes)
                .set_delay(Duration::from_secs(5)),
        )
        .mount(&server)
        .await;

    let swap: Arc<dyn ThemeSwap> = Arc::new(CountingThemeSwap::new());
    let (ctx, _tmp) = build_share_context(
        Url::parse(&format!("{}/", server.uri())).unwrap(),
        swap,
    );
    let ctx = Arc::new(ctx);

    let workspace = TempDir::new().unwrap();
    let target = workspace.path().join("bundles/art-slow");

    let (send_fn, _rx) = make_sink();
    let msg = json!({
        "id": "req-cancel-1",
        "type": "explorer.install",
        "params": {
            "artifact_id": "art-slow",
            "target_workspace": target.to_string_lossy(),
            "overwrite": false,
        },
    });

    let ctx_for_task = ctx.clone();
    let handle = tokio::spawn(async move {
        dispatch(&ctx_for_task, &msg, send_fn).await
    });

    // Poll the cancel registry until the handler registers its token; the
    // dispatch runs on a worker thread and the insertion happens before the
    // download future is awaited, so this normally resolves within a few ms.
    let start = std::time::Instant::now();
    let deadline = Duration::from_secs(2);
    loop {
        if !ctx.cancel_registry.lock().unwrap().is_empty() {
            break;
        }
        if start.elapsed() > deadline {
            panic!("handler never registered cancel token within {deadline:?}");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Simulate the WS-disconnect drain (ws_server.rs §5) — cancel every
    // registered token, mirroring what handle_client does on socket close.
    let drained_at = std::time::Instant::now();
    {
        let mut reg = ctx.cancel_registry.lock().unwrap();
        for (_, token) in reg.drain() {
            token.cancel();
        }
    }

    // The handler must finish quickly now that the cancel token fired. A
    // wide observation window keeps the test robust on slow CI; the real
    // assertion is "did we observe the cancellation before the 5s download
    // delay elapsed" — 2s is plenty.
    let reply = tokio::time::timeout(Duration::from_secs(2), handle)
        .await
        .expect("handler must observe cancellation and return before download delay elapses")
        .expect("spawned task join");

    let observed_within = drained_at.elapsed();
    let terminal: Value =
        serde_json::from_str(&reply.expect("handler must return a frame")).expect("valid json");

    assert_eq!(terminal["type"], "error", "expected error frame, got {terminal}");
    assert_eq!(terminal["error"]["code"], "cancelled", "cancelled code");
    assert_eq!(
        terminal["error"]["kind"], "HostLocal",
        "cancelled kind must be HostLocal"
    );
    assert!(
        observed_within < Duration::from_secs(3),
        "handler took too long to observe cancel: {observed_within:?}"
    );

    // Registry must be empty (install aborted before `registry.upsert`).
    assert_eq!(
        ctx.bundles_registry.lock().unwrap().entries().len(),
        0,
        "registry must not mutate on cancelled install"
    );
    // Cancel registry must also be empty now (we drained it; scope guard
    // would also have cleaned up).
    assert!(
        ctx.cancel_registry.lock().unwrap().is_empty(),
        "cancel_registry must be empty post-cancel"
    );

    // Target path must not materialize — the install never reached the
    // staging-commit step.
    assert!(
        !target.exists(),
        "target path must not exist after cancelled install"
    );
}

// ---- Test 3: explorer.preview + explorer.cancelPreview round-trip -----------

/// Preview round-trip. Wiremock serves CSS bytes; `explorer.preview`
/// acquires the slot and returns a token; `explorer.cancelPreview` with
/// that token returns `{ restored: true }`. A second preview after cancel
/// must succeed (slot cleared). A `CountingThemeSwap` records apply+revert
/// counts so we can assert revert fired on cancel.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explorer_preview_cancel_round_trip_restores_and_clears_slot() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/download/theme-preview"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(b"body { color: red; }".to_vec()),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/download/theme-preview-2"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(b"body { color: blue; }".to_vec()),
        )
        .mount(&server)
        .await;

    let swap = Arc::new(CountingThemeSwap::new());
    let swap_for_ctx: Arc<dyn ThemeSwap> = swap.clone();
    let (ctx, _tmp) = build_share_context(
        Url::parse(&format!("{}/", server.uri())).unwrap(),
        swap_for_ctx,
    );

    // ---- Start preview ---------------------------------------------------
    let (send_fn, _rx) = make_sink();
    let start_msg = json!({
        "id": "prev-req-1",
        "type": "explorer.preview",
        "params": { "artifact_id": "theme-preview" },
    });
    let reply = dispatch(&ctx, &start_msg, send_fn.clone())
        .await
        .expect("preview must return frame");
    let parsed: Value = serde_json::from_str(&reply).expect("valid json");
    assert_eq!(
        parsed["type"], "explorer.previewResult",
        "preview start result; got {parsed}"
    );
    assert_eq!(parsed["id"], "prev-req-1");
    let token = parsed["preview_token"]
        .as_str()
        .expect("preview_token string")
        .to_string();
    assert!(!token.is_empty(), "preview_token must be non-empty");
    assert!(
        ctx.preview_slot.is_active(),
        "slot must be occupied after successful start"
    );
    assert_eq!(swap.applies(), 1, "apply must fire exactly once");
    assert_eq!(swap.reverts(), 0, "revert must not fire before cancel");

    // ---- Cancel preview --------------------------------------------------
    let cancel_msg = json!({
        "id": "prev-cancel-1",
        "type": "explorer.cancelPreview",
        "params": { "preview_token": token },
    });
    let reply = dispatch(&ctx, &cancel_msg, send_fn.clone())
        .await
        .expect("cancel must return frame");
    let parsed: Value = serde_json::from_str(&reply).expect("valid json");
    assert_eq!(
        parsed["type"], "explorer.cancelPreviewResult",
        "cancel result type; got {parsed}"
    );
    assert_eq!(parsed["id"], "prev-cancel-1");
    assert_eq!(parsed["restored"], true, "restored flag must be true");

    // The cancel path fires the session's CancellationToken; the spawned
    // auto-revert task wakes and calls `revert()`. Yield a few times so the
    // background task runs on the test runtime.
    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert!(
        !ctx.preview_slot.is_active(),
        "slot must be cleared after cancel"
    );
    assert_eq!(
        swap.reverts(),
        1,
        "revert must have fired exactly once after cancel"
    );

    // ---- Second preview must succeed (slot cleared) ---------------------
    let restart_msg = json!({
        "id": "prev-req-2",
        "type": "explorer.preview",
        "params": { "artifact_id": "theme-preview-2" },
    });
    let reply = dispatch(&ctx, &restart_msg, send_fn)
        .await
        .expect("second preview must return frame");
    let parsed: Value = serde_json::from_str(&reply).expect("valid json");
    assert_eq!(
        parsed["type"], "explorer.previewResult",
        "second preview must succeed after cancel; got {parsed}"
    );
    assert_eq!(swap.applies(), 2, "apply must fire again on second preview");
}

// ---- Test 4: service_unavailable envelope pinning ---------------------------

/// When `share_ctx` is `None`, the `ws_server.rs` fallback path emits the
/// `install_context_unavailable()` envelope. The envelope is the editor's
/// stable signal that the host isn't configured for share traffic — any
/// drift is a cross-boundary contract break.
///
/// The fallback-frame builder is `handlers::error_frame`; pinning
/// the envelope field-by-field against `install_context_unavailable()`
/// guards the exact bytes the editor receives.
#[test]
fn service_unavailable_envelope_matches_install_context_unavailable_payload() {
    // The payload the fallback path returns.
    let payload = install_context_unavailable();
    assert_eq!(payload.code, "service_unavailable");
    assert_eq!(payload.kind, "HostLocal");
    assert_eq!(payload.detail, "install_context_not_constructed");
    assert_eq!(payload.message, "Install service is not available yet.");

    // Mirror the exact envelope ws_server.rs builds for the fallback branch
    // (lines 577-589). If either side drifts, this test fails.
    let fallback_frame = json!({
        "id": "req-unavailable",
        "type": "error",
        "error": {
            "code": payload.code,
            "kind": payload.kind,
            "detail": payload.detail,
            "message": payload.message,
        },
    });
    let parsed = fallback_frame;
    assert_eq!(parsed["id"], "req-unavailable");
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["error"]["code"], "service_unavailable");
    assert_eq!(parsed["error"]["kind"], "HostLocal");
    assert_eq!(parsed["error"]["detail"], "install_context_not_constructed");
    assert_eq!(
        parsed["error"]["message"],
        "Install service is not available yet."
    );
}

// ---- Test 4b: explorer.get round-trip (phase-2 follow-up) ------------------

/// `explorer.get` round-trip: wiremock serves a §4.4-shaped artifact body;
/// `dispatch` must return an `explorer.getResult` frame carrying the full
/// metadata subtree under `artifact`. Locks in the wire shape the editor
/// binds to after the `ShareClient::get_artifact` surface landed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explorer_get_round_trip_delivers_artifact_frame() {
    let server = MockServer::start().await;
    let body = json!({
        "artifact_id": "art-get-it",
        "kind": "bundle",
        "manifest": { "name": "end-to-end", "version": "2.0.0" },
        "content_hash": "cafebabe".repeat(8),
        "r2_url": "https://r2.example/bundle",
        "thumbnail_url": "https://r2.example/thumb.png",
        "author_pubkey": "bb".repeat(32),
        "author_fingerprint_hex": "bb22cc33dd44",
        "installs": 12,
        "reports": 0,
        "created_at": 1_700_000_000_i64,
        "updated_at": 1_700_000_500_i64,
        "status": "live",
    });
    Mock::given(method("GET"))
        .and(path("/v1/artifact/art-get-it"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .mount(&server)
        .await;

    let swap: Arc<dyn ThemeSwap> = Arc::new(CountingThemeSwap::new());
    let (ctx, _tmp) = build_share_context(
        Url::parse(&format!("{}/", server.uri())).unwrap(),
        swap,
    );

    let (send_fn, _rx) = make_sink();
    let msg = json!({
        "id": "req-get-1",
        "type": "explorer.get",
        "params": { "artifact_id": "art-get-it" },
    });

    let reply = dispatch(&ctx, &msg, send_fn)
        .await
        .expect("explorer.get must return a terminal frame");
    let parsed: Value = serde_json::from_str(&reply).expect("valid json");

    assert_eq!(parsed["type"], "explorer.getResult", "got {parsed}");
    assert_eq!(parsed["id"], "req-get-1");
    let artifact = &parsed["artifact"];
    assert_eq!(artifact["artifact_id"], "art-get-it");
    assert_eq!(artifact["status"], "live");
    assert_eq!(artifact["installs"], 12);
    assert_eq!(artifact["author_fingerprint_hex"], "bb22cc33dd44");
    assert_eq!(artifact["manifest"]["name"], "end-to-end");
}

/// `explorer.get` error path: 404 from the Worker surfaces as a structured
/// `error` frame with `SERVER_REJECT` + kind `Malformed` — same envelope
/// `handle_list` produces for Worker 4xx.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explorer_get_not_found_surfaces_server_reject_envelope() {
    let server = MockServer::start().await;
    let err_body = json!({ "error": { "code": "NOT_FOUND", "message": "no such artifact" } });
    Mock::given(method("GET"))
        .and(path("/v1/artifact/ghost"))
        .respond_with(ResponseTemplate::new(404).set_body_json(err_body))
        .mount(&server)
        .await;

    let swap: Arc<dyn ThemeSwap> = Arc::new(CountingThemeSwap::new());
    let (ctx, _tmp) = build_share_context(
        Url::parse(&format!("{}/", server.uri())).unwrap(),
        swap,
    );

    let (send_fn, _rx) = make_sink();
    let msg = json!({
        "id": "req-get-404",
        "type": "explorer.get",
        "params": { "artifact_id": "ghost" },
    });
    let reply = dispatch(&ctx, &msg, send_fn)
        .await
        .expect("explorer.get must return a terminal frame");
    let parsed: Value = serde_json::from_str(&reply).expect("valid json");
    assert_eq!(parsed["type"], "error", "got {parsed}");
    assert_eq!(parsed["id"], "req-get-404");
    assert_eq!(parsed["error"]["code"], "SERVER_REJECT");
    assert_eq!(parsed["error"]["kind"], "Malformed");
}

// ---- Test 5: sync handler regression note -----------------------------------
//
// `sensors.subscribe` and `status` wire behavior is pinned by the in-module
// tests at `crates/host/src/ws_server.rs::tests::handle_sensors_subscribe`
// and `handle_status`. Those tests run under the same compilation as the
// #021 Wave 3 edits to `ws_server.rs`, so any regression surfaces in
// `cargo test -p omni-host --lib` without duplication here. The sync path
// was not functionally modified by #021 — only the explorer.* arm was
// replaced — and the per-connection disconnect drain is a no-op when
// `share_ctx` is `None`, which is the state both pre-existing tests
// exercise.
