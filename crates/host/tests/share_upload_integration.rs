//! Host upload pipeline end-to-end integration test (umbrella §8.2).
//!
//! Crosses TWO boundaries per umbrella §8.1:
//! * HTTPS process boundary — via `wiremock` Mock HTTP server
//! * Crate boundary — exercises omni-identity::sign_http_jws, omni-bundle, omni-sanitize
//!
//! What is mocked: the Worker (Cloudflare) — we assert the exact HTTP request the
//! host would send to a real Worker, including the JWS envelope shape.
//!
//! What is NOT mocked: omni-identity, omni-bundle, omni-sanitize — real crates.
//!
//! `StubGuard` (default, non-feature `guard` build) keeps CI free of the private crate.

use std::sync::Arc;

use omni_guard_trait::{Guard, StubGuard};
use test_harness::deterministic_keypair;
use omni_host::share::{
    client::{ListParams, ShareClient, SANITIZE_VERSION},
    progress::UploadProgress,
    upload::{upload, ArtifactKind, UploadRequest},
};
use serde_json::json;
use tokio::sync::mpsc;
use url::Url;
use wiremock::matchers::{header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn test_request(src: &std::path::Path) -> UploadRequest {
    UploadRequest {
        kind: ArtifactKind::Theme,
        source_path: src.to_path_buf(),
        name: "neon".into(),
        description: "test".into(),
        tags: vec![],
        license: "MIT".into(),
        version: "1.0.0".parse().unwrap(),
        omni_min_version: "0.1.0".parse().unwrap(),
        update_artifact_id: None,
    }
}

fn stub_guard() -> Arc<dyn Guard> {
    Arc::new(StubGuard) as Arc<dyn Guard>
}

fn write_theme(dir: &std::path::Path, name: &str, body: &[u8]) -> std::path::PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn limits_body() -> serde_json::Value {
    json!({
        "max_bundle_compressed": 5_242_880u64,
        "max_bundle_uncompressed": 10_485_760u64,
        "max_entries": 32,
        "version": 1,
        "updated_at": 0
    })
}

#[tokio::test]
#[ignore = "requires Ultralight resources; run with --ignored after placing resources in target/debug/deps/"]
async fn happy_path_upload_emits_jws_header_and_progress() {
    // 1. Spin up mock Worker
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/config/limits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(limits_body()))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/upload"))
        .and(header_exists("Authorization"))
        .respond_with(|req: &Request| {
            // Assert Authorization starts with "Omni-JWS "
            let auth = req.headers.get("Authorization").unwrap().to_str().unwrap();
            assert!(auth.starts_with("Omni-JWS "), "bad auth header: {auth}");
            let sv = req
                .headers
                .get("X-Omni-Sanitize-Version")
                .unwrap()
                .to_str()
                .unwrap();
            assert_eq!(sv, SANITIZE_VERSION.to_string());
            ResponseTemplate::new(200).set_body_json(json!({
                "artifact_id": "abc123",
                "content_hash": "deadbeef",
                "r2_url": "https://r2.test/abc123",
                "thumbnail_url": "https://r2.test/abc123.png",
                "created_at": 1_760_000_000u64,
                "status": "created"
            }))
        })
        .mount(&server)
        .await;

    // 2. Fixture workspace
    let dir = tempfile::tempdir().unwrap();
    let css_path = write_theme(dir.path(), "theme.css", b":root { --omni-accent: #f0f; }\n");

    // 3. Build host-side infrastructure
    let identity = Arc::new(deterministic_keypair());
    let guard = stub_guard();
    let base = Url::parse(&server.uri()).unwrap();
    let client = Arc::new(ShareClient::new(base, identity.clone(), guard.clone()));

    // 4. Drive the upload with a progress channel
    let (tx, mut rx) = mpsc::channel::<UploadProgress>(64);
    let pump = tokio::spawn(async move {
        let mut phases: Vec<String> = Vec::new();
        while let Some(ev) = rx.recv().await {
            if let Some(w) = ev.to_wire() {
                phases.push(w.phase.to_string());
            }
            if matches!(ev, UploadProgress::Done { .. }) {
                break;
            }
        }
        phases
    });

    let result = upload(test_request(&css_path), guard, identity, client, tx)
        .await
        .expect("upload success");

    // 5. Assertions
    assert_eq!(result.artifact_id, "abc123");
    assert_eq!(result.content_hash, "deadbeef");
    let phases = pump.await.unwrap();
    assert!(phases.iter().any(|p| p == "pack"));
    assert!(phases.iter().any(|p| p == "upload"));
}

#[tokio::test]
#[ignore = "requires Ultralight resources; run with --ignored after placing resources in target/debug/deps/"]
async fn rate_limited_retries_once_then_succeeds() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/config/limits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(limits_body()))
        .mount(&server)
        .await;

    // First POST gets 429 with Retry-After: 0; second succeeds
    Mock::given(method("POST"))
        .and(path("/v1/upload"))
        .respond_with(
            ResponseTemplate::new(429)
                .insert_header("Retry-After", "0")
                .set_body_json(json!({ "error": {
                    "code": "RATE_LIMITED",
                    "message": "slow down",
                    "kind": "Quota",
                    "retry_after": 0
                }})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/upload"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "artifact_id": "r1",
            "content_hash": "h",
            "r2_url": "",
            "thumbnail_url": "",
            "created_at": 0,
            "status": "created"
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let p = write_theme(dir.path(), "t.css", b"/* empty */");

    let identity = Arc::new(deterministic_keypair());
    let guard = stub_guard();
    let client = Arc::new(ShareClient::new(
        Url::parse(&server.uri()).unwrap(),
        identity.clone(),
        guard.clone(),
    ));
    let (tx, _rx) = mpsc::channel(8);
    let res = upload(test_request(&p), guard, identity, client, tx)
        .await
        .expect("retry ok");
    assert_eq!(res.artifact_id, "r1");
}

#[tokio::test]
#[ignore = "requires Ultralight resources; run with --ignored after placing resources in target/debug/deps/"]
async fn auth_bad_signature_is_not_retried() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/config/limits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(limits_body()))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/upload"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": { "code": "AUTH_BAD_SIGNATURE", "kind": "Auth", "message": "bad sig" }
        })))
        .expect(1) // MUST be called exactly once
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let p = write_theme(dir.path(), "t.css", b"/* empty */");

    let identity = Arc::new(deterministic_keypair());
    let guard = stub_guard();
    let client = Arc::new(ShareClient::new(
        Url::parse(&server.uri()).unwrap(),
        identity.clone(),
        guard.clone(),
    ));
    let (tx, _rx) = mpsc::channel(8);
    let err = upload(test_request(&p), guard, identity, client, tx)
        .await
        .expect_err("should fail");
    match err {
        omni_host::share::error::UploadError::ServerReject { kind, .. } => {
            assert_eq!(kind, omni_host::share::error::WorkerErrorKind::Auth)
        }
        e => panic!("wrong variant: {e:?}"),
    }
}

// Regression guard: upload_inner must fail-fast on limits check before
// invoking pack_only's thumbnail render. Ensures malicious or mistaken
// oversized uploads don't burn GPU/CPU unnecessarily. If this test ever
// needs #[ignore] for "requires Ultralight," the limits→pack_only
// ordering in upload_inner has regressed.
#[tokio::test]
async fn oversized_bundle_returns_bad_input_before_hitting_server() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/config/limits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "max_bundle_compressed": 16u64,  // ridiculously low
            "max_bundle_uncompressed": 32u64,
            "max_entries": 32,
            "version": 1,
            "updated_at": 0
        })))
        .mount(&server)
        .await;
    // If the host tries to POST, fail the test.
    Mock::given(method("POST"))
        .and(path("/v1/upload"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let p = write_theme(
        dir.path(),
        "t.css",
        b":root { --a: #ffffff; --b: #000000; } /* plenty of bytes */",
    );

    let identity = Arc::new(deterministic_keypair());
    let guard = stub_guard();
    let client = Arc::new(ShareClient::new(
        Url::parse(&server.uri()).unwrap(),
        identity.clone(),
        guard.clone(),
    ));
    let (tx, _rx) = mpsc::channel(8);
    let err = upload(test_request(&p), guard, identity, client, tx)
        .await
        .expect_err("should fail");
    assert!(matches!(
        err,
        omni_host::share::error::UploadError::BadInput { .. }
    ));
}

/// After a successful upload, the post-upload cache (spec §7) must surface the
/// newly-created artifact on a follow-up `list` even when the mock Worker's
/// list endpoint returns an empty page — D1/KV eventual consistency workaround.
#[tokio::test]
#[ignore = "requires Ultralight resources; run with --ignored after placing resources in target/debug/deps/"]
async fn cache_entry_visible_in_followup_list() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/config/limits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(limits_body()))
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/v1/upload"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "artifact_id": "cache-me",
            "content_hash": "cafe",
            "r2_url": "https://r2.test/cache-me",
            "thumbnail_url": "https://r2.test/cache-me.png",
            "created_at": 1_760_000_000u64,
            "status": "created"
        })))
        .mount(&server)
        .await;

    // Worker list returns empty — simulates KV not yet converged.
    Mock::given(method("GET"))
        .and(path("/v1/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [],
            "next_cursor": null
        })))
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let p = write_theme(dir.path(), "t.css", b":root { --a: 1; }");

    let identity = Arc::new(deterministic_keypair());
    let guard = stub_guard();
    let client = Arc::new(ShareClient::new(
        Url::parse(&server.uri()).unwrap(),
        identity.clone(),
        guard.clone(),
    ));

    let (tx, _rx) = mpsc::channel(8);
    let res = upload(test_request(&p), guard, identity, client.clone(), tx)
        .await
        .expect("upload ok");
    assert_eq!(res.artifact_id, "cache-me");

    // Follow-up list — server returns zero items, cache must surface "cache-me".
    let listed = client
        .list(ListParams {
            kind: Some("theme".into()),
            sort: None,
            tag: vec![],
            cursor: None,
            limit: None,
            author_pubkey: None,
        })
        .await
        .expect("list ok");
    assert!(
        listed.items.iter().any(|d| d.artifact_id == "cache-me"),
        "cached artifact missing from merged list: {:?}",
        listed
            .items
            .iter()
            .map(|d| &d.artifact_id)
            .collect::<Vec<_>>()
    );
}
