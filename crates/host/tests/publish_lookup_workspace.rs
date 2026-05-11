//! Integration tests for `publish.lookupWorkspace` covering all three
//! terminal statuses (ok / missing_index / missing_folder), bad params,
//! and forward-compat tolerance of unknown `kind` values.
//!
//! Driven through the real `dispatch` entrypoint (same pattern as
//! `handle_list_search.rs`) so we exercise the WS routing surface, not
//! just the inner handler function.
//!
//! Note on test isolation: the handler reads the publish-index from
//! `config::data_dir()`, which resolves via `$APPDATA` (Windows) or
//! falls back to `"."`. Since env vars are process-global and tests run
//! in parallel by default, every test acquires a single process-wide
//! Mutex, points `$APPDATA` at its own `TempDir`, and roots the
//! `ShareContext.data_dir` at that same `$APPDATA/Omni` path so the
//! index lookup and folder probes hit the same on-disk root.

use omni_host::share::publish_index::{self, PublishIndex, PublishIndexEntry};
use omni_host::share::ws_messages::{dispatch, ShareContext};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use tempfile::TempDir;

// ---- harness -----------------------------------------------------------

/// Process-wide lock that serializes APPDATA mutations across the test
/// binary's parallel tokio runtimes. Without this, two tests racing on
/// `std::env::set_var("APPDATA", ...)` would read each other's tempdirs.
fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

/// RAII guard: holds the global env lock for the lifetime of the test,
/// points `$APPDATA` at `tmp.path()`, and returns both the resolved
/// `data_dir` (= `tmp/Omni`, where `config::data_dir()` reads the index)
/// and a built `ShareContext` rooted at the same path.
struct TestEnv {
    _guard: MutexGuard<'static, ()>,
    data_dir: PathBuf,
    ctx: ShareContext,
}

fn setup(tmp: &TempDir) -> TestEnv {
    let guard = env_lock();
    // SAFETY: serialized by `env_lock` across this test binary.
    std::env::set_var("APPDATA", tmp.path());
    let data_dir = tmp.path().join("Omni");
    std::fs::create_dir_all(&data_dir).expect("mkdir data_dir");
    let ctx = test_harness::build_share_context(&data_dir);
    TestEnv {
        _guard: guard,
        data_dir,
        ctx,
    }
}

/// Drive a single WS frame through `dispatch` and return the sync reply.
/// Mirrors `handle_list_search.rs:dispatch_one`.
async fn dispatch_one(ctx: &ShareContext, msg: Value) -> Value {
    let send_fn = move |_s: String| {};
    let reply = dispatch(ctx, &msg, send_fn)
        .await
        .expect("dispatch returns a synchronous reply frame");
    serde_json::from_str(&reply).expect("reply is valid JSON")
}

fn seed_index(env: &TestEnv, entries: Vec<PublishIndexEntry>) {
    // Mirror the handler: `config::data_dir().join(INDEX_FILENAME)`.
    let path = env.data_dir.join(publish_index::INDEX_FILENAME);
    let idx = PublishIndex { entries };
    publish_index::write(&path, &idx).expect("write publish-index");
}

fn entry(artifact_id: &str, kind: &str, name: &str) -> PublishIndexEntry {
    PublishIndexEntry {
        pubkey_hex: "abc".into(),
        kind: kind.into(),
        name: name.into(),
        artifact_id: artifact_id.into(),
        last_version: "1.0.0".into(),
        last_published_at: "2026-05-11T00:00:00Z".into(),
    }
}

// ==== tests =============================================================

#[tokio::test]
async fn returns_missing_index_when_publish_index_empty() {
    let tmp = TempDir::new().unwrap();
    let env = setup(&tmp);
    let reply = dispatch_one(
        &env.ctx,
        json!({
            "id": "r1",
            "type": "publish.lookupWorkspace",
            "params": { "artifact_id": "never-published" }
        }),
    )
    .await;
    assert_eq!(reply["type"], "publish.lookupWorkspaceResult");
    assert_eq!(reply["status"], "missing_index");
    assert!(reply["workspace_path"].is_null());
    assert!(reply["kind"].is_null());
    assert!(reply["name"].is_null());
}

#[tokio::test]
async fn returns_missing_folder_when_index_has_entry_but_disk_empty() {
    let tmp = TempDir::new().unwrap();
    let env = setup(&tmp);
    seed_index(&env, vec![entry("A", "overlay", "hwmon")]);
    // Intentionally do NOT mkdir overlays/hwmon.
    let reply = dispatch_one(
        &env.ctx,
        json!({
            "id": "r1",
            "type": "publish.lookupWorkspace",
            "params": { "artifact_id": "A" }
        }),
    )
    .await;
    assert_eq!(reply["status"], "missing_folder");
    assert_eq!(reply["kind"], "overlay");
    assert_eq!(reply["name"], "hwmon");
    assert!(reply["workspace_path"].is_null());
}

#[tokio::test]
async fn returns_ok_for_overlay_kind() {
    let tmp = TempDir::new().unwrap();
    let env = setup(&tmp);
    seed_index(&env, vec![entry("A", "overlay", "hwmon")]);
    std::fs::create_dir_all(env.data_dir.join("overlays").join("hwmon")).unwrap();
    let reply = dispatch_one(
        &env.ctx,
        json!({
            "id": "r1",
            "type": "publish.lookupWorkspace",
            "params": { "artifact_id": "A" }
        }),
    )
    .await;
    assert_eq!(reply["status"], "ok");
    assert_eq!(reply["workspace_path"], "overlays/hwmon");
    assert_eq!(reply["kind"], "overlay");
}

#[tokio::test]
async fn returns_ok_for_theme_kind() {
    let tmp = TempDir::new().unwrap();
    let env = setup(&tmp);
    seed_index(&env, vec![entry("T", "theme", "neon")]);
    std::fs::create_dir_all(env.data_dir.join("themes").join("neon")).unwrap();
    let reply = dispatch_one(
        &env.ctx,
        json!({
            "id": "r1",
            "type": "publish.lookupWorkspace",
            "params": { "artifact_id": "T" }
        }),
    )
    .await;
    assert_eq!(reply["status"], "ok");
    assert_eq!(reply["workspace_path"], "themes/neon");
    assert_eq!(reply["kind"], "theme");
}

#[tokio::test]
async fn returns_bad_input_for_missing_artifact_id() {
    let tmp = TempDir::new().unwrap();
    let env = setup(&tmp);
    let reply = dispatch_one(
        &env.ctx,
        json!({
            "id": "r1",
            "type": "publish.lookupWorkspace",
            "params": {}
        }),
    )
    .await;
    // bad_input frame uses "error" envelope per the existing convention
    // in handle_list_search.rs and ws_messages.rs.
    assert!(reply.get("error").is_some(), "expected error envelope, got: {reply}");
}

#[tokio::test]
async fn returns_bad_input_for_empty_artifact_id() {
    let tmp = TempDir::new().unwrap();
    let env = setup(&tmp);
    let reply = dispatch_one(
        &env.ctx,
        json!({
            "id": "r1",
            "type": "publish.lookupWorkspace",
            "params": { "artifact_id": "" }
        }),
    )
    .await;
    assert!(reply.get("error").is_some(), "expected error envelope, got: {reply}");
}

#[tokio::test]
async fn unknown_kind_falls_through_to_missing_index() {
    let tmp = TempDir::new().unwrap();
    let env = setup(&tmp);
    seed_index(&env, vec![entry("F", "some-future-kind", "anything")]);
    let reply = dispatch_one(
        &env.ctx,
        json!({
            "id": "r1",
            "type": "publish.lookupWorkspace",
            "params": { "artifact_id": "F" }
        }),
    )
    .await;
    assert_eq!(reply["status"], "missing_index");
}
