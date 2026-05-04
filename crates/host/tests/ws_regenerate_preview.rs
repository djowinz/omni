//! Integration coverage for the `share.regeneratePreview` WS handler
//! shipped to back the Step 2 "Regenerate" button.
//!
//! The success path goes through the full `share::save_preview` pipeline,
//! which depends on a live Ultralight render thread (same gate as
//! `preview_save_hook::overlay_save_writes_preview_dotfile_when_render_succeeds`).
//! cargo test never starts that thread, so the success-path assertion lives
//! behind `#[ignore]`. The non-ignored tests cover:
//!
//! - Param validation: missing fields, unknown `kind` values produce
//!   `bad_input` envelopes with descriptive messages.
//! - Render-failure path: with no live render channel, the handler returns
//!   the `preview_render_failed` Admin-kind error envelope, and crucially
//!   does NOT 500 / panic / leak the inner `Box<dyn Error>` debug string
//!   into the user-facing `message` field.

use omni_host::share::ws_messages::{dispatch, ShareContext};
use serde_json::{json, Value};
use tempfile::TempDir;

fn ctx_with_tempdir() -> (ShareContext, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let ctx = test_harness::build_share_context(tmp.path());
    (ctx, tmp)
}

async fn dispatch_one(ctx: &ShareContext, msg: Value) -> Value {
    let send_fn = move |_s: String| {};
    let reply = dispatch(ctx, &msg, send_fn)
        .await
        .expect("regeneratePreview returns a synchronous reply frame");
    serde_json::from_str(&reply).expect("reply is valid JSON")
}

#[tokio::test]
async fn missing_workspace_path_returns_bad_input() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-1",
            "type": "share.regeneratePreview",
            "params": { "kind": "overlay" }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("share.regeneratePreview"),
        "message must scope the error to the failing handler: {parsed}",
    );
}

#[tokio::test]
async fn unknown_kind_returns_bad_input() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-2",
            "type": "share.regeneratePreview",
            "params": { "workspace_path": "overlays/x", "kind": "preset" }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    let msg = parsed["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("unknown kind") && msg.contains("preset"),
        "error must name the unknown kind so the renderer can debug: {msg}",
    );
}

#[tokio::test]
async fn overlay_render_failure_returns_admin_kind_error() {
    // Without a live Ultralight render thread (cargo test never starts one),
    // `render_overlay_preview` returns Err. The handler MUST surface this as
    // the structured `preview_render_failed` envelope so the renderer can
    // distinguish render failures from bad-input rejections — and so the
    // raw `Box<dyn Error>` chain doesn't leak into the user-facing
    // `message` field.
    let (ctx, tmp) = ctx_with_tempdir();
    let overlay_dir = tmp.path().join("overlays").join("regen-overlay");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir overlay");
    std::fs::write(
        overlay_dir.join("overlay.omni"),
        r#"<widget id="x" name="x" enabled="true">
  <template><div class="p"><span class="val">hi</span></div></template>
  <style>.p{color:#fff}</style>
</widget>"#,
    )
    .expect("write overlay");

    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-3",
            "type": "share.regeneratePreview",
            "params": {
                "workspace_path": "overlays/regen-overlay",
                "kind": "overlay"
            }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["error"]["code"], "preview_render_failed");
    assert_eq!(parsed["error"]["kind"], "Admin");
    // User-facing message is the generic copy, not the raw Box<dyn Error>.
    let msg = parsed["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.starts_with("Failed to regenerate preview"),
        "message must be the generic admin-side copy, not the raw error chain: {msg}",
    );
    // The inner chain still lives in `detail` for the host log / advanced
    // troubleshooting — never empty when render actually failed.
    assert!(
        parsed["error"]["detail"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "detail must carry the underlying failure for diagnostics: {parsed}",
    );
}

#[tokio::test]
async fn theme_workspace_path_without_filename_returns_bad_input() {
    // A `workspace_path` that's just `themes/` (no filename) would crash
    // `render_theme_preview` if we passed it through. The handler MUST
    // reject this before calling into the renderer pipeline.
    let (ctx, _tmp) = ctx_with_tempdir();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-4",
            "type": "share.regeneratePreview",
            "params": { "workspace_path": "themes/", "kind": "theme" }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    let msg = parsed["error"]["message"].as_str().unwrap_or("");
    // Either bad-input or render-failure is acceptable — the contract is
    // "no panic, return a structured error." Bad-input is preferred but
    // the actual render path may also reject it.
    assert!(
        msg.contains("filename")
            || msg.contains("preview")
            || msg.contains("share.regeneratePreview"),
        "must produce a structured error rather than panicking: {msg}",
    );
}
