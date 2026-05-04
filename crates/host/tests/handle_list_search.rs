//! share-explorer-redesign Task 2: end-to-end test that handle_list parses
//! the `q` field from the inbound WS frame, truncates to 64 chars, and
//! forwards it through to the outbound /v1/list URL.
//!
//! Construction discipline (per writing-lessons §D7): ShareContext is built
//! via `test_harness::build_share_context` and the client is rebuilt against
//! the wiremock URL — same pattern as `ws_identity_handlers.rs`.

use std::sync::Arc;

use omni_host::share::client::ShareClient;
use omni_host::share::ws_messages::{dispatch, ShareContext};
use serde_json::{json, Value};
use tempfile::TempDir;
use url::Url;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---- harness -----------------------------------------------------------

/// Build a ShareContext with the client pointing at `worker_url`.
fn ctx_with_worker(worker_url: &str) -> (ShareContext, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let mut ctx = test_harness::build_share_context(tmp.path());
    let url = Url::parse(worker_url).unwrap();
    let client = Arc::new(ShareClient::new(url, ctx.identity.clone(), ctx.guard.clone()));
    ctx.client = client;
    (ctx, tmp)
}

/// Drive a single message through `dispatch` and return the sync reply
/// frame parsed as JSON. `handle_list` is a synchronous-reply handler
/// so the send_fn sink stays unused.
async fn dispatch_one(ctx: &ShareContext, msg: Value) -> Value {
    let send_fn = move |_s: String| {};
    let reply = dispatch(ctx, &msg, send_fn)
        .await
        .expect("dispatch returns a synchronous reply frame");
    serde_json::from_str(&reply).expect("reply is valid JSON")
}

// ==== handle_list_forwards_q_to_worker ==================================

#[tokio::test]
async fn handle_list_forwards_q_to_worker() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/list"))
        .and(query_param("q", "Marathon"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [],
            "next_cursor": null,
        })))
        .mount(&server)
        .await;

    let (ctx, _tmp) = ctx_with_worker(&server.uri());
    let reply = dispatch_one(
        &ctx,
        json!({
            "id": "r1",
            "type": "explorer.list",
            "params": { "q": "Marathon" }
        }),
    )
    .await;
    assert_eq!(reply["type"], "explorer.listResult");
}

// ==== handle_list_truncates_q_at_64_chars ================================

#[tokio::test]
async fn handle_list_truncates_q_at_64_chars() {
    let server = MockServer::start().await;
    let truncated = "x".repeat(64);
    Mock::given(method("GET"))
        .and(path("/v1/list"))
        .and(query_param("q", truncated.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [],
            "next_cursor": null,
        })))
        .mount(&server)
        .await;

    let (ctx, _tmp) = ctx_with_worker(&server.uri());
    let too_long = "x".repeat(200);
    let reply = dispatch_one(
        &ctx,
        json!({
            "id": "r2",
            "type": "explorer.list",
            "params": { "q": too_long }
        }),
    )
    .await;
    // The mock only matches exactly 64 x's — if handle_list forwarded
    // more than 64, wiremock would fall through and the reply would be
    // an error frame. Asserting listResult confirms the truncation happened.
    assert_eq!(reply["type"], "explorer.listResult");
}
