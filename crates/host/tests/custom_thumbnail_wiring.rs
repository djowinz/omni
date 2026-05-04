//! Integration coverage for the user-supplied custom thumbnail wiring on the
//! `upload.pack` / `upload.publish` / `upload.update` WS surface.
//!
//! The renderer base64-encodes the user-uploaded Step 2 Preview Image and
//! ships it under `custom_preview_b64`. The host:
//!   1. Decodes + size-caps it (`decode_custom_preview` in `ws_messages.rs`).
//!   2. Re-runs the moderation gate on the bytes (host-side gate is
//!      authoritative per INV-7.7.2 — renderer-side moderation is advisory).
//!   3. On accept, uses the bytes as the artifact's R2 thumbnail in place of
//!      the auto-rendered Ultralight output.
//!   4. On reject (or any moderation failure), returns a structured
//!      `BadInput`-flavored error envelope rather than silently uploading
//!      either the user's flagged image OR the auto-render the user thought
//!      they had replaced.
//!
//! These tests exercise the WS-handler-side parameter parsing and the
//! pack-pipeline branch selection. The success-with-real-image path requires
//! the bundled ONNX moderation models loaded next to the test executable
//! (same gate as `preview_save_hook` and `save_preview_wiring` ignored
//! tests); without those, `share::moderation::check_image` returns
//! `CheckError::NotInitialized` which the wiring surfaces as a `BadInput`
//! with `"moderation failed"` — that's actually the perfect proof that the
//! custom-thumbnail branch is being taken (the auto-render path doesn't
//! consult moderation).

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use omni_host::share::ws_messages::{dispatch, ShareContext};
use serde_json::{json, Value};
use tempfile::TempDir;

fn ctx_with_tempdir() -> (ShareContext, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let ctx = test_harness::build_share_context(tmp.path());
    (ctx, tmp)
}

async fn dispatch_collect(ctx: &ShareContext, msg: Value) -> Vec<Value> {
    use std::sync::{Arc, Mutex};
    let frames: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let frames_for_send = frames.clone();
    let send_fn = move |s: String| {
        let parsed: Value = serde_json::from_str(&s).expect("frame is valid JSON");
        frames_for_send.lock().unwrap().push(parsed);
    };
    let sync_reply = dispatch(ctx, &msg, send_fn).await;
    if let Some(reply) = sync_reply {
        let parsed: Value = serde_json::from_str(&reply).expect("reply is valid JSON");
        frames.lock().unwrap().push(parsed);
    }
    let v = frames.lock().unwrap().clone();
    v
}

#[tokio::test]
async fn upload_pack_oversized_custom_preview_returns_bad_input() {
    // Server cap is 2 MB; ship 3 MB and assert structured rejection.
    let (ctx, tmp) = ctx_with_tempdir();
    std::fs::create_dir_all(tmp.path().join("overlays").join("x")).unwrap();
    let huge = vec![0u8; 3 * 1024 * 1024];
    let b64 = B64.encode(&huge);

    let frames = dispatch_collect(
        &ctx,
        json!({
            "id": "req-pack-toobig",
            "type": "upload.pack",
            "params": {
                "workspace_path": "overlays/x",
                "kind": "bundle",
                "custom_preview_b64": b64,
            }
        }),
    )
    .await;
    let last = frames.last().expect("got at least one reply");
    assert_eq!(last["type"], "error");
    let msg = last["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("custom_preview_b64") && msg.contains("server cap"),
        "must reject oversized payload at decode_custom_preview, got: {msg}",
    );
}

#[tokio::test]
async fn upload_pack_garbage_custom_preview_returns_bad_input() {
    // Non-base64 bytes — must be rejected before the pack pipeline runs so
    // no thumbnail-render side effects fire on bad input.
    let (ctx, tmp) = ctx_with_tempdir();
    std::fs::create_dir_all(tmp.path().join("overlays").join("x")).unwrap();
    let frames = dispatch_collect(
        &ctx,
        json!({
            "id": "req-pack-garbage",
            "type": "upload.pack",
            "params": {
                "workspace_path": "overlays/x",
                "kind": "bundle",
                "custom_preview_b64": "***not-valid-base64***",
            }
        }),
    )
    .await;
    let last = frames.last().expect("got at least one reply");
    assert_eq!(last["type"], "error");
    let msg = last["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("base64 decode failed"),
        "must surface the decode error to help the renderer debug, got: {msg}",
    );
}

#[tokio::test]
async fn upload_pack_with_no_custom_preview_takes_legacy_path() {
    // Sanity: omitting custom_preview_b64 keeps the legacy auto-render path.
    // Without an actual overlay folder + Ultralight, the pack pipeline will
    // fail somewhere downstream — the assertion is "the custom-preview gate
    // does NOT fire," NOT "the pack succeeds end-to-end." A successful
    // smoke would require Ultralight resources (the existing
    // preview_save_hook integration covers that).
    let (ctx, tmp) = ctx_with_tempdir();
    std::fs::create_dir_all(tmp.path().join("overlays").join("nonexistent")).unwrap();
    let frames = dispatch_collect(
        &ctx,
        json!({
            "id": "req-pack-no-custom",
            "type": "upload.pack",
            "params": {
                "workspace_path": "overlays/nonexistent",
                "kind": "bundle",
            }
        }),
    )
    .await;
    let last = frames.last().expect("got at least one reply");
    let msg = last["error"]["message"].as_str().unwrap_or("");
    // Whatever fails should NOT be the custom-preview path — it should be
    // the legacy "missing overlay.omni" or similar pack failure.
    assert!(
        !msg.contains("custom_preview_b64") && !msg.contains("custom thumbnail"),
        "no custom_preview_b64 was sent, the failure must not mention it: {msg}",
    );
}
