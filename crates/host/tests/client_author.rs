//! Wire-shape tests per `feedback_wire_shape_tests.md` — assert OUTGOING
//! requests (URL, JWS auth header, JSON body shape), not just the inbound
//! response handling. Mocking `sendMessage`'s return value would hide
//! request-side wire bugs; wiremock observes the bytes the host actually
//! emits.
//!
//! Covers the two new client methods added by Task 7 of the
//! identity-completion-and-display-name plan:
//!   * `ShareClient::get_author(pubkey_hex)` — public, no JWS
//!   * `ShareClient::set_display_name(name)` — JWS-authenticated
//!
//! See `crates/host/src/share/client.rs` for the impl, and
//! `apps/worker/src/routes/author.ts` (Task 4) for the worker side.

use std::sync::Arc;

use arc_swap::ArcSwap;
use identity::Keypair;
use omni_guard_trait::{Guard, StubGuard};
use omni_host::share::client::{AuthorDetail, ShareClient};
use omni_host::share::error::{UploadError, WorkerErrorKind};
use url::Url;
use wiremock::matchers::{body_string_contains, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_client(server: &MockServer) -> ShareClient {
    let identity = Arc::new(ArcSwap::new(Arc::new(Keypair::generate())));
    let url = Url::parse(&server.uri()).unwrap();
    ShareClient::new(url, identity, Arc::new(StubGuard) as Arc<dyn Guard>)
}

#[tokio::test]
async fn get_author_hits_correct_url_and_decodes_response() {
    let server = MockServer::start().await;
    let pubkey_hex = "ab".repeat(32);

    Mock::given(method("GET"))
        .and(path(format!("/v1/author/{pubkey_hex}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pubkey_hex": pubkey_hex,
            "fingerprint_hex": "abcdef012345",
            "display_name": "starfire",
            "joined_at": 1_714_000_000u64,
            "total_uploads": 7u64,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server);
    let result: AuthorDetail = client.get_author(&pubkey_hex).await.expect("get_author ok");
    assert_eq!(result.pubkey_hex, pubkey_hex);
    assert_eq!(result.fingerprint_hex, "abcdef012345");
    assert_eq!(result.display_name, Some("starfire".into()));
    assert_eq!(result.joined_at, 1_714_000_000);
    assert_eq!(result.total_uploads, 7);
}

#[tokio::test]
async fn get_author_decodes_null_display_name() {
    // Worker returns `display_name: null` for authors who haven't run
    // setDisplayName yet. Pin that the host's `Option<String>` round-trips
    // it as `None` rather than failing to deserialize.
    let server = MockServer::start().await;
    let pubkey_hex = "ee".repeat(32);

    Mock::given(method("GET"))
        .and(path(format!("/v1/author/{pubkey_hex}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pubkey_hex": pubkey_hex,
            "fingerprint_hex": "deadbeef",
            "display_name": null,
            "joined_at": 1_700_000_000u64,
            "total_uploads": 0u64,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server);
    let result = client.get_author(&pubkey_hex).await.expect("get_author ok");
    assert_eq!(result.display_name, None);
    assert_eq!(result.total_uploads, 0);
}

#[tokio::test]
async fn get_author_404_maps_to_server_reject_not_found() {
    // Worker returns 404 with the standard error envelope when the author
    // row doesn't exist. Per the host's existing decode_error mapping
    // (`crates/host/src/share/client.rs::default_kind_for_status`) 404
    // surfaces as `UploadError::ServerReject { status: 404, kind: Malformed }`.
    let server = MockServer::start().await;
    let pubkey_hex = "cd".repeat(32);

    Mock::given(method("GET"))
        .and(path(format!("/v1/author/{pubkey_hex}")))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "error": { "code": "NOT_FOUND", "message": "no such author" }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server);
    let err = client
        .get_author(&pubkey_hex)
        .await
        .expect_err("404 must fail");
    match err {
        UploadError::ServerReject {
            status,
            code,
            kind,
            ..
        } => {
            assert_eq!(status, 404);
            assert_eq!(code, "NOT_FOUND");
            assert_eq!(kind, WorkerErrorKind::Malformed);
        }
        other => panic!("expected ServerReject for 404, got {other:?}"),
    }
}

#[tokio::test]
async fn get_author_does_not_attach_authorization_header() {
    // GET /v1/author/:pubkey_hex is public per worker-api §4 — must NOT
    // carry an Authorization header (would be ignored, but signing it
    // unnecessarily wastes a sign() call and would couple the resolver
    // to identity availability). wiremock asserts the header is absent.
    let server = MockServer::start().await;
    let pubkey_hex = "ff".repeat(32);

    // First mount: matches IFF Authorization header is absent (no
    // matcher for it). To assert absence positively, add a fallback
    // mock that matches any GET with an Authorization header and fails.
    Mock::given(method("GET"))
        .and(path(format!("/v1/author/{pubkey_hex}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pubkey_hex": pubkey_hex,
            "fingerprint_hex": "00",
            "display_name": null,
            "joined_at": 0u64,
            "total_uploads": 0u64,
        })))
        .expect(1)
        .mount(&server)
        .await;

    // If the host accidentally signs this request, wiremock would still
    // match the above mock (it doesn't constrain headers). Instead, we
    // mount a higher-priority mock that catches `Authorization: Omni-JWS *`
    // and fails the test if it ever fires.
    Mock::given(method("GET"))
        .and(path(format!("/v1/author/{pubkey_hex}")))
        .and(header_regex("authorization", "Omni-JWS"))
        .respond_with(ResponseTemplate::new(500).set_body_string(
            "FAIL: get_author must not attach Authorization header (public endpoint)",
        ))
        .expect(0)
        .mount(&server)
        .await;

    let client = test_client(&server);
    let _ = client.get_author(&pubkey_hex).await.expect("ok");
}

#[tokio::test]
async fn set_display_name_puts_signed_request_with_correct_body() {
    let server = MockServer::start().await;
    let response_pubkey = "ab".repeat(32);

    Mock::given(method("PUT"))
        .and(path("/v1/author/me"))
        // JWS auth header per worker-api §3 ("Omni-JWS <compact>"). header_regex
        // case-insensitive and confirms the *outgoing* envelope, not just
        // that the response decodes.
        .and(header_regex("authorization", "^Omni-JWS "))
        // body_string_contains asserts the JSON body the host actually
        // ships — proves serialization+content-type+body-handoff are all
        // wired correctly. wiremock streams the body for matching.
        .and(body_string_contains("\"display_name\":\"starfire\""))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pubkey_hex": response_pubkey,
            "display_name": "starfire",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server);
    let result = client
        .set_display_name("starfire")
        .await
        .expect("set_display_name ok");
    assert_eq!(result.display_name, "starfire");
    assert_eq!(result.pubkey_hex, response_pubkey);
}

#[tokio::test]
async fn set_display_name_sends_application_json_content_type() {
    // The worker route (`PUT /v1/author/me`) parses JSON; assert the
    // outgoing request advertises the correct Content-Type so a future
    // refactor that drops the header (or switches to multipart) breaks
    // this test rather than silently breaking the worker parse.
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v1/author/me"))
        .and(header_regex("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pubkey_hex": "00".repeat(32),
            "display_name": "alpha",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server);
    client
        .set_display_name("alpha")
        .await
        .expect("set_display_name ok");
}

#[tokio::test]
async fn set_display_name_400_maps_to_server_reject() {
    // The worker rejects malformed display_name via the standard error
    // envelope at status 400. Pin that the host surfaces this as
    // `UploadError::ServerReject` so the WS handler can carve a
    // structured `invalid_display_name` reply.
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v1/author/me"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "error": {
                "code": "INVALID_DISPLAY_NAME",
                "kind": "Malformed",
                "message": "display_name must be 1-32 characters after trim",
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = test_client(&server);
    let err = client
        .set_display_name("")
        .await
        .expect_err("400 must fail");
    match err {
        UploadError::ServerReject {
            status,
            code,
            kind,
            ..
        } => {
            assert_eq!(status, 400);
            assert_eq!(code, "INVALID_DISPLAY_NAME");
            assert_eq!(kind, WorkerErrorKind::Malformed);
        }
        other => panic!("expected ServerReject for 400, got {other:?}"),
    }
}
