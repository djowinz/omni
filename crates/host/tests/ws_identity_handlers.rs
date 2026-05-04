//! Integration tests for the WS identity handlers shipped by Task 10 of
//! the 2026-04-26 identity-completion-and-display-name plan:
//! `identity.show` / `identity.import` / `identity.rotate` /
//! `identity.markBackedUp` / `identity.setDisplayName`.
//!
//! Construction discipline (per writing-lessons §D7): every test goes
//! through `test_harness::factories::build_share_context(tmp.path())`
//! and overrides only the `client` field for tests that need a wiremock
//! Worker. Building a `ShareContext` ad-hoc here would diverge from
//! production wiring and let drift slip in.
//!
//! Coverage matrix (mirror of plan steps 10.1, 10.5, 10.8, 10.10, 10.12):
//!   identity.show:           happy path returns persisted metadata
//!   identity.markBackedUp:   happy + path-empty + timestamp-drift rejection
//!   identity.setDisplayName: happy (with NFC + trim) + length rejection
//!                            + control-char rejection + worker-PUT shape
//!   identity.rotate:         carries display_name + clears backup state
//!                            + persists rotation timestamp
//!   identity.import:         swaps active keypair + resets metadata
//!                            + overwrite_existing=false rejects mismatched key

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use identity::Keypair;
use omni_host::share::client::ShareClient;
use omni_host::share::identity_metadata::IdentityMetadata;
use omni_host::share::ws_messages::{dispatch, ShareContext};
use serde_json::{json, Value};
use tempfile::TempDir;
use url::Url;
use wiremock::matchers::{body_json, method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ---- harness -----------------------------------------------------------

/// Build a [`ShareContext`] rooted at a fresh tempdir, with no worker
/// configured. Tests that need a worker URL build their own mock first
/// and call [`with_worker`].
fn ctx_with_tempdir() -> (ShareContext, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let ctx = test_harness::build_share_context(tmp.path());
    (ctx, tmp)
}

/// Wire a wiremock worker base URL into an existing `ShareContext` by
/// rebuilding its `ShareClient` against the new URL while preserving
/// the same `Arc<ArcSwap<Keypair>>` so the slot a test mutates from
/// the outside is what the client actually signs with.
fn ctx_with_worker(worker_url: &str) -> (ShareContext, TempDir) {
    let tmp = TempDir::new().expect("tempdir");
    let mut ctx = test_harness::build_share_context(tmp.path());
    let url = Url::parse(worker_url).unwrap();
    let client = Arc::new(ShareClient::new(url, ctx.identity.clone(), ctx.guard.clone()));
    ctx.client = client;
    (ctx, tmp)
}

/// Drive a single message through `dispatch` and return the sync reply
/// frame parsed as JSON. Identity handlers are all sync-reply (no
/// progress streaming), so the `send_fn` sink stays unused.
async fn dispatch_one(ctx: &ShareContext, msg: Value) -> Value {
    let send_fn = move |_s: String| {};
    let reply = dispatch(ctx, &msg, send_fn)
        .await
        .expect("identity handler returns a synchronous reply frame");
    serde_json::from_str(&reply).expect("reply is valid JSON")
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// ==== identity.show =====================================================

#[tokio::test]
async fn show_returns_persisted_backed_up_and_display_name() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let pk_hex = ctx.identity.load().public_key().to_hex();

    let meta = IdentityMetadata {
        pubkey_hex: pk_hex.clone(),
        display_name: Some("starfire".into()),
        backed_up: true,
        last_backed_up_at: Some(1_714_000_000),
        last_rotated_at: Some(1_713_000_000),
        last_backup_path: Some("C:\\Users\\foo\\identity.omniid".into()),
    };
    IdentityMetadata::save(&ctx.identity_metadata_path(), &meta).unwrap();

    let parsed = dispatch_one(
        &ctx,
        json!({ "id": "req-show", "type": "identity.show", "params": {} }),
    )
    .await;
    assert_eq!(parsed["type"], "identity.showResult");
    assert_eq!(parsed["params"]["pubkey_hex"], pk_hex);
    assert_eq!(parsed["params"]["display_name"], "starfire");
    assert_eq!(parsed["params"]["backed_up"], true);
    assert_eq!(parsed["params"]["last_backed_up_at"], 1_714_000_000);
    assert_eq!(parsed["params"]["last_rotated_at"], 1_713_000_000);
    // Fingerprint sub-fields are populated (non-empty); the actual
    // values are pinned by `crates/identity/src/fingerprint.rs` tests.
    assert!(parsed["params"]["fingerprint_hex"].as_str().unwrap().len() == 12);
    assert_eq!(
        parsed["params"]["fingerprint_words"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        parsed["params"]["fingerprint_emoji"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
}

#[tokio::test]
async fn show_defaults_when_metadata_absent() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let parsed = dispatch_one(
        &ctx,
        json!({ "id": "req-show-2", "type": "identity.show", "params": {} }),
    )
    .await;
    assert_eq!(parsed["type"], "identity.showResult");
    assert_eq!(parsed["params"]["display_name"], Value::Null);
    assert_eq!(parsed["params"]["backed_up"], false);
    assert_eq!(parsed["params"]["last_backed_up_at"], Value::Null);
    assert_eq!(parsed["params"]["last_rotated_at"], Value::Null);
}

// ==== identity.markBackedUp =============================================

#[tokio::test]
async fn mark_backed_up_persists_to_metadata() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let pk_hex = ctx.identity.load().public_key().to_hex();

    let now = now_secs();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-mb",
            "type": "identity.markBackedUp",
            "params": {
                "path": "C:\\Users\\foo\\identity.omniid",
                "timestamp": now,
            }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "identity.markBackedUpResult");
    assert_eq!(parsed["params"]["ok"], true);

    let meta = IdentityMetadata::load_or_default(&ctx.identity_metadata_path(), &pk_hex);
    assert!(meta.backed_up);
    assert_eq!(meta.last_backed_up_at, Some(now));
    assert_eq!(
        meta.last_backup_path.as_deref(),
        Some("C:\\Users\\foo\\identity.omniid")
    );
}

#[tokio::test]
async fn mark_backed_up_rejects_implausible_timestamp() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-mb-2",
            "type": "identity.markBackedUp",
            "params": { "path": "C:\\foo.omniid", "timestamp": 0 }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    let msg = parsed["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("±1 day"),
        "error message should reference ±1 day drift; got {msg:?}"
    );
}

#[tokio::test]
async fn mark_backed_up_rejects_empty_path() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let now = now_secs();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-mb-3",
            "type": "identity.markBackedUp",
            "params": { "path": "", "timestamp": now }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    let msg = parsed["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("path must not be empty"),
        "error message should mention empty path; got {msg:?}"
    );
}

// ==== identity.setDisplayName ===========================================

#[tokio::test]
async fn set_display_name_persists_normalized_form() {
    // Mock worker accepts the PUT and echoes the persisted name.
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v1/author/me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pubkey_hex": "ab".repeat(32),
            "display_name": "starfire",
        })))
        .mount(&server)
        .await;

    let (ctx, _tmp) = ctx_with_worker(&format!("{}/", server.uri()));
    let pk_hex = ctx.identity.load().public_key().to_hex();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-sdn",
            "type": "identity.setDisplayName",
            "params": { "display_name": "  starfire  " }
        }),
    )
    .await;

    assert_eq!(parsed["type"], "identity.setDisplayNameResult");
    assert_eq!(parsed["params"]["display_name"], "starfire");
    assert_eq!(parsed["params"]["pubkey_hex"], pk_hex);

    let meta = IdentityMetadata::load_or_default(&ctx.identity_metadata_path(), &pk_hex);
    assert_eq!(meta.display_name.as_deref(), Some("starfire"));
}

#[tokio::test]
async fn set_display_name_persists_locally_even_if_worker_fails() {
    // Worker returns 500 — local persist must still land so the editor
    // can re-trigger; the next upload's COALESCE will catch up.
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v1/author/me"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let (ctx, _tmp) = ctx_with_worker(&format!("{}/", server.uri()));
    let pk_hex = ctx.identity.load().public_key().to_hex();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-sdn-2",
            "type": "identity.setDisplayName",
            "params": { "display_name": "starfire" }
        }),
    )
    .await;

    // Handler still returns success for the editor's UI purposes;
    // local persist already happened.
    assert_eq!(parsed["type"], "identity.setDisplayNameResult");
    let meta = IdentityMetadata::load_or_default(&ctx.identity_metadata_path(), &pk_hex);
    assert_eq!(meta.display_name.as_deref(), Some("starfire"));
}

#[tokio::test]
async fn set_display_name_rejects_too_long() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-sdn-3",
            "type": "identity.setDisplayName",
            "params": { "display_name": "x".repeat(33) }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["error"]["code"], "invalid_display_name");
    assert_eq!(parsed["error"]["kind"], "Malformed");
}

#[tokio::test]
async fn set_display_name_rejects_empty_after_trim() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-sdn-4",
            "type": "identity.setDisplayName",
            "params": { "display_name": "    " }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["error"]["code"], "invalid_display_name");
}

#[tokio::test]
async fn set_display_name_rejects_control_characters() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-sdn-5",
            "type": "identity.setDisplayName",
            "params": { "display_name": "star\u{0007}fire" }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["error"]["code"], "invalid_display_name");
    assert!(parsed["error"]["message"]
        .as_str()
        .unwrap()
        .contains("control"));
}

#[tokio::test]
async fn set_display_name_accepts_emoji() {
    // Astral-plane emoji is 1 code point per spec §3.4 (pinned 2026-04-27);
    // 32 emoji must pass the validator.
    let server = MockServer::start().await;
    // body_json + .expect(1): asserts the astral-plane emoji is preserved
    // verbatim through the wire (no UTF-16 truncation, no surrogate
    // splitting). Without `.expect(1)` an unmatched body would 404 and
    // the test would still pass because local persist runs first.
    let valid_input: String = "😀".repeat(32);
    Mock::given(method("PUT"))
        .and(path("/v1/author/me"))
        .and(body_json(json!({ "display_name": valid_input })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pubkey_hex": "x".repeat(64),
            "display_name": valid_input,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (ctx, _tmp) = ctx_with_worker(&format!("{}/", server.uri()));
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-sdn-6",
            "type": "identity.setDisplayName",
            "params": { "display_name": valid_input }
        }),
    )
    .await;
    assert_eq!(
        parsed["type"], "identity.setDisplayNameResult",
        "32 astral-plane emoji should pass: each is 1 code point"
    );

    // 33 emoji must fail validation locally (no wire call).
    let parsed_long = dispatch_one(
        &ctx,
        json!({
            "id": "req-sdn-7",
            "type": "identity.setDisplayName",
            "params": { "display_name": "😀".repeat(33) }
        }),
    )
    .await;
    assert_eq!(parsed_long["type"], "error");
    // Verification fires when `server` drops: the mock MUST have been
    // hit exactly once (the 32-emoji input); the 33-emoji input is
    // rejected before the worker PUT, so it does not contribute.
}

// ==== identity.rotate ===================================================

#[tokio::test]
async fn rotate_carries_display_name_and_clears_backup_state() {
    // Mock worker for the post-rotate set_display_name PUT — body shape
    // is asserted via body_json + .expect(1) so a regression in how
    // rotate carries the display_name forward (e.g., dropping NFC,
    // truncating, or sending under a different key) hard-fails the test
    // when the MockServer drops.
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v1/author/me"))
        .and(body_json(json!({ "display_name": "djowinz" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pubkey_hex": "00".repeat(32),  // not validated by handler
            "display_name": "djowinz",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let (ctx, _tmp) = ctx_with_worker(&format!("{}/", server.uri()));
    let pk1 = ctx.identity.load().public_key().to_hex();

    // Pre-state: backed up + named.
    let meta_path = ctx.identity_metadata_path();
    let meta_pre = IdentityMetadata {
        pubkey_hex: pk1.clone(),
        display_name: Some("djowinz".into()),
        backed_up: true,
        last_backed_up_at: Some(1_714_000_000),
        last_rotated_at: None,
        last_backup_path: Some("C:\\old.omniid".into()),
    };
    IdentityMetadata::save(&meta_path, &meta_pre).unwrap();

    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-rotate",
            "type": "identity.rotate",
            "params": {}
        }),
    )
    .await;

    assert_eq!(parsed["type"], "identity.rotateResult");
    let new_pk_hex = parsed["params"]["pubkey_hex"].as_str().unwrap();
    let new_fp_hex = parsed["params"]["fingerprint_hex"].as_str().unwrap();
    assert_ne!(new_pk_hex, pk1, "rotate must produce a new pubkey");
    assert_eq!(new_fp_hex.len(), 12, "fingerprint_hex is 6 bytes hex-encoded");

    // ctx.identity now reflects the rotated key.
    assert_eq!(ctx.identity.load().public_key().to_hex(), new_pk_hex);

    // Post-state metadata: display_name carried, backup cleared,
    // last_rotated_at populated.
    let meta_post = IdentityMetadata::load_or_default(&meta_path, new_pk_hex);
    assert_eq!(
        meta_post.display_name.as_deref(),
        Some("djowinz"),
        "display_name carries forward across rotation"
    );
    assert!(!meta_post.backed_up, "backed_up must clear on rotate");
    assert_eq!(meta_post.last_backed_up_at, None);
    assert_eq!(meta_post.last_backup_path, None);
    assert!(
        meta_post.last_rotated_at.is_some(),
        "last_rotated_at must be set"
    );

    // The post-rotate set_display_name PUT runs in `tokio::spawn`
    // (handler returns the rotateResult before the network call lands).
    // Give the background task a chance to fire so the wiremock `expect(1)`
    // verification (on `server` drop at end of scope) sees the request.
    tokio::time::sleep(Duration::from_millis(200)).await;
}

#[tokio::test]
async fn rotate_writes_new_key_to_disk() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let key_path = ctx.identity_key_path();

    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-rotate-2",
            "type": "identity.rotate",
            "params": {}
        }),
    )
    .await;
    assert_eq!(parsed["type"], "identity.rotateResult");

    // identity.key is on disk and `load_or_create` recovers the same key.
    assert!(key_path.exists(), "identity.key must be on disk after rotate");
    let loaded = Keypair::load_or_create(&key_path).unwrap();
    assert_eq!(
        loaded.public_key().to_hex(),
        ctx.identity.load().public_key().to_hex(),
        "on-disk key matches the active in-memory key after rotate"
    );
}

// ==== identity.import ===================================================

#[tokio::test]
async fn import_swaps_active_keypair_and_resets_metadata() {
    // Worker returns 404 for the new pubkey (nobody has uploaded under
    // it yet), so display_name in metadata stays None.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/author/.*"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": { "code": "NOT_FOUND", "message": "no such author" }
        })))
        .mount(&server)
        .await;

    let (ctx, _tmp) = ctx_with_worker(&format!("{}/", server.uri()));
    let original_pk = ctx.identity.load().public_key().to_hex();

    // Build a backup of an entirely fresh keypair (the donor — what
    // the user is "importing from another machine").
    let donor = Keypair::generate();
    let donor_pk_hex = donor.public_key().to_hex();
    let backup = donor.export_encrypted("very-long-passphrase").unwrap();
    let b64 = B64.encode(&backup);

    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-import",
            "type": "identity.import",
            "params": {
                "encrypted_bytes_b64": b64,
                "passphrase": "very-long-passphrase",
                "overwrite_existing": true,
            }
        }),
    )
    .await;

    assert_eq!(parsed["type"], "identity.importResult");
    assert_eq!(parsed["params"]["pubkey_hex"], donor_pk_hex);
    assert_ne!(donor_pk_hex, original_pk, "import must change active pubkey");

    // Active in-memory keypair is the donor.
    assert_eq!(ctx.identity.load().public_key().to_hex(), donor_pk_hex);

    // identity.key on disk is the donor (load_or_create reads the same key).
    let loaded = Keypair::load_or_create(&ctx.identity_key_path()).unwrap();
    assert_eq!(loaded.public_key().to_hex(), donor_pk_hex);

    // Metadata is seeded for the new pubkey. display_name stays None
    // because the worker 404'd. backed_up MUST be true: the user just
    // supplied the encrypted backup bytes, so the identity is already
    // backed up by definition — anything else puts imported users into
    // an infinite first-publish backup-gate loop.
    let meta = IdentityMetadata::load_or_default(&ctx.identity_metadata_path(), &donor_pk_hex);
    assert_eq!(meta.pubkey_hex, donor_pk_hex);
    assert_eq!(meta.display_name, None);
    assert!(
        meta.backed_up,
        "imported identity must be marked backed_up=true; user demonstrably possesses the backup"
    );
    assert!(
        meta.last_backed_up_at.is_some(),
        "imported identity must record last_backed_up_at; the import operation IS a backup event"
    );
}

#[tokio::test]
async fn import_seeds_display_name_from_worker_when_present() {
    let donor = Keypair::generate();
    let donor_pk_hex = donor.public_key().to_hex();
    let backup = donor.export_encrypted("good-passphrase").unwrap();
    let b64 = B64.encode(&backup);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/author/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pubkey_hex": donor_pk_hex,
            "fingerprint_hex": "00".repeat(6),
            "display_name": "alpha",
            "joined_at": 1_700_000_000_u64,
            "total_uploads": 3_u64,
        })))
        .mount(&server)
        .await;

    let (ctx, _tmp) = ctx_with_worker(&format!("{}/", server.uri()));

    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-import-seed",
            "type": "identity.import",
            "params": {
                "encrypted_bytes_b64": b64,
                "passphrase": "good-passphrase",
                "overwrite_existing": true,
            }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "identity.importResult");

    let meta = IdentityMetadata::load_or_default(&ctx.identity_metadata_path(), &donor_pk_hex);
    assert_eq!(meta.display_name.as_deref(), Some("alpha"));
}

#[tokio::test]
async fn import_rejects_when_overwrite_false_and_existing_differs() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let original_pk = ctx.identity.load().public_key().to_hex();

    // Donor differs from the active key.
    let donor = Keypair::generate();
    let backup = donor.export_encrypted("pw").unwrap();
    let b64 = B64.encode(&backup);

    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-import-refuse",
            "type": "identity.import",
            "params": {
                "encrypted_bytes_b64": b64,
                "passphrase": "pw",
                "overwrite_existing": false,
            }
        }),
    )
    .await;

    assert_eq!(parsed["type"], "error");
    assert_eq!(parsed["error"]["code"], "identity_already_exists");

    // Active keypair is unchanged.
    assert_eq!(ctx.identity.load().public_key().to_hex(), original_pk);
}

#[tokio::test]
async fn import_rejects_bad_passphrase() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let donor = Keypair::generate();
    let backup = donor.export_encrypted("right-passphrase").unwrap();
    let b64 = B64.encode(&backup);

    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-import-bad-pw",
            "type": "identity.import",
            "params": {
                "encrypted_bytes_b64": b64,
                "passphrase": "wrong-passphrase",
                "overwrite_existing": true,
            }
        }),
    )
    .await;

    assert_eq!(parsed["type"], "error");
    let msg = parsed["error"]["message"].as_str().unwrap_or("");
    assert!(
        msg.contains("import failed"),
        "bad passphrase should surface as bad-input error; got {msg:?}"
    );
}

#[tokio::test]
async fn import_rejects_bad_base64() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-import-b64",
            "type": "identity.import",
            "params": {
                "encrypted_bytes_b64": "***not-base64***",
                "passphrase": "anything",
                "overwrite_existing": true,
            }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "error");
    let msg = parsed["error"]["message"].as_str().unwrap_or("");
    assert!(msg.contains("base64"), "expected base64 error; got {msg:?}");
}

// ==== Wire-shape regression — outgoing PUT body matches contract ========

/// `feedback_wire_shape_tests.md`: assert the OUTGOING request body
/// shape, not just that the handler dispatches. Catches regressions
/// where a refactor changes `set_display_name`'s body keys silently.
#[tokio::test]
async fn set_display_name_emits_expected_put_body() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/v1/author/me"))
        .and(body_json(json!({ "display_name": "wired" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pubkey_hex": "ab".repeat(32),
            "display_name": "wired",
        })))
        // `.expect(1)` is the load-bearing assertion here: without it
        // an unmatched mock would return a default 404 and the handler
        // would STILL surface `identity.setDisplayNameResult` because
        // local persist runs before the worker PUT (per spec ordering).
        // With `.expect(1)`, wiremock verifies on `MockServer` drop
        // that the PUT actually fired with the expected body shape and
        // hard-fails the test if a refactor drifts the body keys.
        .expect(1)
        .mount(&server)
        .await;

    let (ctx, _tmp) = ctx_with_worker(&format!("{}/", server.uri()));
    let parsed = dispatch_one(
        &ctx,
        json!({
            "id": "req-wire",
            "type": "identity.setDisplayName",
            "params": { "display_name": "wired" }
        }),
    )
    .await;
    assert_eq!(parsed["type"], "identity.setDisplayNameResult");
    // Verification fires when `server` drops at the end of this scope.
}

// ==== ArcSwap retargeting — keypair swap reaches client signers =========

/// Independent assertion that the rotate handler's `ctx.identity.store`
/// is observed by the embedded `ShareClient` (they share the outer
/// Arc<ArcSwap<Keypair>>). Lock-free swap behavior is more
/// thoroughly tested in `crates/host/src/share/client.rs::tests` —
/// this is a coarse end-to-end pin.
#[tokio::test]
async fn rotate_retargets_share_client_signing_key() {
    let (ctx, _tmp) = ctx_with_tempdir();
    let pre = ctx.identity.load().public_key().to_hex();

    let parsed = dispatch_one(
        &ctx,
        json!({ "id": "req-r", "type": "identity.rotate", "params": {} }),
    )
    .await;
    assert_eq!(parsed["type"], "identity.rotateResult");
    let post = ctx.identity.load().public_key().to_hex();
    assert_ne!(pre, post);
}

/// Compile-only sanity: a brand-new `Arc<ArcSwap<Keypair>>` cloned
/// from `ctx.identity` reads the same key after `ctx.identity.store(...)`.
/// Prevents future regressions where someone replaces the inner
/// representation with something that doesn't share the outer Arc.
#[test]
fn shared_arc_swap_observes_external_store() {
    let kp1 = Keypair::generate();
    let pk1 = kp1.public_key().0;
    let outer: Arc<ArcSwap<Keypair>> = Arc::new(ArcSwap::new(Arc::new(kp1)));
    let cloned = outer.clone();

    let kp2 = Keypair::generate();
    let pk2 = kp2.public_key().0;
    outer.store(Arc::new(kp2));

    assert_eq!(cloned.load().public_key().0, pk2);
    assert_ne!(pk1, pk2);
}
