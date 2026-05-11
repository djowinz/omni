//! Integration tests for `publish.lookupWorkspace` covering all three
//! terminal statuses (ok / missing_index / missing_folder), bad params,
//! and forward-compat tolerance of unknown `kind` values.
//!
//! Driven through the real `dispatch` entrypoint (same pattern as
//! `handle_list_search.rs`) so we exercise the WS routing surface, not
//! just the inner handler function.
//!
//! The handler reads the publish-index from `ctx.data_dir`, matching
//! how it probes the on-disk overlays/themes folders. Each test gets
//! its own `TempDir` + `ShareContext` rooted there — no env-var hackery,
//! no shared global state, parallel-safe.

use omni_host::share::publish_index::{self, PublishIndex, PublishIndexEntry};
use omni_host::share::ws_messages::{dispatch, ShareContext};
use serde_json::{json, Value};
use tempfile::TempDir;

// ---- harness (mirrors handle_list_search.rs:22-40) ---------------------

fn build_ctx(tmp: &TempDir) -> ShareContext {
    test_harness::build_share_context(tmp.path())
}

/// Drive a single WS frame through `dispatch` and return the sync reply.
async fn dispatch_one(ctx: &ShareContext, msg: Value) -> Value {
    let send_fn = move |_s: String| {};
    let reply = dispatch(ctx, &msg, send_fn)
        .await
        .expect("dispatch returns a synchronous reply frame");
    serde_json::from_str(&reply).expect("reply is valid JSON")
}

fn seed_index(tmp: &TempDir, entries: Vec<PublishIndexEntry>) {
    let path = tmp.path().join(publish_index::INDEX_FILENAME);
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
    let ctx = build_ctx(&tmp);
    let reply = dispatch_one(
        &ctx,
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
    let ctx = build_ctx(&tmp);
    seed_index(&tmp, vec![entry("A", "overlay", "hwmon")]);
    // Intentionally do NOT mkdir overlays/hwmon.
    let reply = dispatch_one(
        &ctx,
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
    let ctx = build_ctx(&tmp);
    seed_index(&tmp, vec![entry("A", "overlay", "hwmon")]);
    std::fs::create_dir_all(tmp.path().join("overlays").join("hwmon")).unwrap();
    let reply = dispatch_one(
        &ctx,
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
    let ctx = build_ctx(&tmp);
    seed_index(&tmp, vec![entry("T", "theme", "neon")]);
    std::fs::create_dir_all(tmp.path().join("themes").join("neon")).unwrap();
    let reply = dispatch_one(
        &ctx,
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
    let ctx = build_ctx(&tmp);
    let reply = dispatch_one(
        &ctx,
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
    let ctx = build_ctx(&tmp);
    let reply = dispatch_one(
        &ctx,
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
    let ctx = build_ctx(&tmp);
    seed_index(&tmp, vec![entry("F", "some-future-kind", "anything")]);
    let reply = dispatch_one(
        &ctx,
        json!({
            "id": "r1",
            "type": "publish.lookupWorkspace",
            "params": { "artifact_id": "F" }
        }),
    )
    .await;
    assert_eq!(reply["status"], "missing_index");
}
