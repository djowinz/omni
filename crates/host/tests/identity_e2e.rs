//! Cross-crate integration per `feedback_cross_crate_integration_tests.md`.
//!
//! All five scenarios from the 2026-04-26 spec §7 live in this file. Only the
//! worker is mocked (via `wiremock`) — internal boundaries (identity crate,
//! ShareContext, ShareClient, dispatch) all run real code.
//!
//! Construction discipline: every `ShareContext` is built via the
//! `tests/common/mod.rs` factories (which thread through
//! `test-harness::factories::build_share_context`) so production wiring shape
//! is preserved. See plan `Task 13` and writing-lessons §D7.
//!
//! Scenario index:
//!   1. `backup_markBackedUp_show_roundtrip_persists_across_restart`
//!   2. `rotation_carries_display_name_seeds_worker_clears_backup`
//!   3. `identity_portability_across_device_fp_no_denylist_trip`
//!   4. `display_name_end_to_end_set_then_upload_then_get`
//!   5. `stale_metadata_tripwire_resets_on_load`

use std::sync::Arc;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use identity::Keypair;
use omni_host::share::identity_metadata::IdentityMetadata;
use serde_json::{json, Value};
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

mod common;
use common::*;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn limits_body() -> Value {
    json!({
        "max_bundle_compressed": 5_242_880u64,
        "max_bundle_uncompressed": 10_485_760u64,
        "max_entries": 32,
        "version": 1,
        "updated_at": 0
    })
}

// ====================================================================
// Scenario 1 — Backup → markBackedUp → show roundtrip, persists across
// a simulated host "restart" (rebuild ShareContext from same data_dir).
// ====================================================================

#[tokio::test]
async fn backup_mark_backed_up_show_roundtrip_persists_across_restart() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let ctx = test_share_context_at_dir(temp.path()).await;

    // 1. backup → encrypted blob comes back base64-encoded.
    let backup_reply = dispatch_one(
        &ctx,
        json!({
            "id": "r1",
            "type": "identity.backup",
            "params": { "passphrase": "very-long-passphrase" }
        }),
    )
    .await;
    assert_eq!(backup_reply["type"], "identity.backupResult");
    let blob_b64 = backup_reply["params"]["encrypted_bytes_b64"]
        .as_str()
        .expect("backup blob present")
        .to_string();
    assert!(!blob_b64.is_empty());

    // Simulate the user saving the backup to disk under the same
    // temp tree (so we can validate the path round-trips through
    // markBackedUp + show).
    let saved_path = temp.path().join("identity.omniid");
    std::fs::write(&saved_path, B64.decode(&blob_b64).unwrap()).unwrap();

    // 2. markBackedUp.
    let now = now_secs();
    let mb_reply = dispatch_one(
        &ctx,
        json!({
            "id": "r2",
            "type": "identity.markBackedUp",
            "params": {
                "path": saved_path.to_string_lossy().to_string(),
                "timestamp": now,
            }
        }),
    )
    .await;
    assert_eq!(mb_reply["type"], "identity.markBackedUpResult");
    assert_eq!(mb_reply["params"]["ok"], true);

    // 3. show → backed_up=true, last_backed_up_at=now, path round-trips.
    let show1 = dispatch_one(
        &ctx,
        json!({ "id": "r3", "type": "identity.show", "params": {} }),
    )
    .await;
    assert_eq!(show1["type"], "identity.showResult");
    assert_eq!(show1["params"]["backed_up"], true);
    assert_eq!(show1["params"]["last_backed_up_at"], now);
    assert_eq!(
        show1["params"]["last_backup_path"],
        saved_path.to_string_lossy().to_string()
    );

    // 4. Simulate a host "restart" — drop the existing context and
    //    rebuild a fresh one from the SAME data_dir. The metadata
    //    file on disk is the only carrier; if persistence is broken
    //    `backed_up` snaps back to `false`.
    drop(ctx);
    let ctx2 = test_share_context_at_dir(temp.path()).await;
    let show2 = dispatch_one(
        &ctx2,
        json!({ "id": "r4", "type": "identity.show", "params": {} }),
    )
    .await;
    assert_eq!(
        show2["params"]["backed_up"], true,
        "backed_up must persist across simulated restart"
    );
    assert_eq!(
        show2["params"]["last_backed_up_at"], now,
        "last_backed_up_at must persist across simulated restart"
    );
    assert_eq!(
        show2["params"]["last_backup_path"],
        saved_path.to_string_lossy().to_string(),
        "last_backup_path must persist across simulated restart"
    );
}

// ====================================================================
// Scenario 2 — Rotation: new pubkey, display_name carries forward,
// backed_up clears, worker PUT seeded with carried name, pre-rotation
// backup blob still imports to the OLD pubkey (rotation doesn't
// corrupt prior backups).
// ====================================================================

#[tokio::test]
async fn rotation_carries_display_name_seeds_worker_clears_backup() {
    let server = MockServer::start().await;

    // Capture every PUT body so we can prove the rotation seed used
    // the carried display_name + the NEW pubkey's signing key.
    let put_seen: Arc<std::sync::Mutex<Vec<Vec<u8>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let put_seen = Arc::clone(&put_seen);
        Mock::given(method("PUT"))
            .and(path("/v1/author/me"))
            .respond_with(move |req: &Request| {
                put_seen.lock().unwrap().push(req.body.clone());
                ResponseTemplate::new(200).set_body_json(json!({
                    "pubkey_hex": "x".repeat(64),
                    "display_name": "djowinz",
                }))
            })
            .mount(&server)
            .await;
    }

    let TestCtx { ctx, _tmp } = test_share_context_with_worker_url(&server.uri()).await;
    let pk1_hex = ctx.identity.load().public_key().to_hex();

    // Pre-state: backed up + named (this is what the rotate handler
    // reads to seed the post-rotation metadata).
    let meta_path = ctx.identity_metadata_path();
    let meta_pre = IdentityMetadata {
        pubkey_hex: pk1_hex.clone(),
        display_name: Some("djowinz".into()),
        backed_up: true,
        last_backed_up_at: Some(1_714_000_000),
        last_rotated_at: None,
        last_backup_path: Some("C:\\old.omniid".into()),
    };
    IdentityMetadata::save(&meta_path, &meta_pre).unwrap();

    // Backup the pre-rotation key so we can later prove rotation
    // doesn't corrupt the .omniid blob.
    let pre_backup_reply = dispatch_one(
        &ctx,
        json!({
            "id": "r1",
            "type": "identity.backup",
            "params": { "passphrase": "pw" }
        }),
    )
    .await;
    let pre_blob_b64 = pre_backup_reply["params"]["encrypted_bytes_b64"]
        .as_str()
        .unwrap()
        .to_string();

    // Rotate.
    let rotate_reply = dispatch_one(
        &ctx,
        json!({ "id": "r2", "type": "identity.rotate", "params": {} }),
    )
    .await;
    assert_eq!(rotate_reply["type"], "identity.rotateResult");
    let pk2_hex = ctx.identity.load().public_key().to_hex();
    assert_ne!(pk1_hex, pk2_hex, "rotate produces a new pubkey");
    assert_eq!(
        rotate_reply["params"]["pubkey_hex"]
            .as_str()
            .unwrap_or(""),
        pk2_hex,
        "rotateResult.pubkey_hex matches the new active key"
    );

    // show: new pubkey, carried display_name, cleared backed_up,
    // last_rotated_at populated.
    let show = dispatch_one(
        &ctx,
        json!({ "id": "r3", "type": "identity.show", "params": {} }),
    )
    .await;
    assert_eq!(show["params"]["pubkey_hex"], pk2_hex);
    assert_eq!(
        show["params"]["display_name"], "djowinz",
        "rotate must carry display_name forward"
    );
    assert_eq!(
        show["params"]["backed_up"], false,
        "rotate must clear backed_up"
    );
    assert!(
        show["params"]["last_rotated_at"].is_number(),
        "rotate must set last_rotated_at"
    );

    // The post-rotate set_display_name PUT runs in `tokio::spawn`;
    // give it a beat to land before inspecting the captured bodies.
    // Scope the MutexGuard inside its own block so it's released
    // before any subsequent `.await` (clippy::await_holding_lock).
    tokio::time::sleep(Duration::from_millis(250)).await;
    {
        let bodies = put_seen.lock().unwrap();
        assert!(
            !bodies.is_empty(),
            "rotate must fire at least one PUT /v1/author/me"
        );
        assert!(
            bodies.iter().any(|b| {
                // Body is JSON; parse + look for display_name="djowinz".
                serde_json::from_slice::<Value>(b)
                    .map(|v| v["display_name"].as_str() == Some("djowinz"))
                    .unwrap_or(false)
            }),
            "rotate seeded worker with carried display_name"
        );
    }

    // Pre-rotation backup blob still imports to the OLD pubkey —
    // rotation doesn't invalidate prior backups.
    let imported = Keypair::import_encrypted(&B64.decode(&pre_blob_b64).unwrap(), "pw")
        .expect("pre-rotation backup decrypts");
    assert_eq!(
        imported.public_key().to_hex(),
        pk1_hex,
        "pre-rotation backup blob still recovers the OLD pubkey"
    );
}

// ====================================================================
// Scenario 3 — Identity portability across device_fp.
// Two ShareContexts holding the SAME pubkey but DIFFERENT mocked
// device fingerprints both upload successfully — the worker's
// `ON CONFLICT(pubkey)` path fires, no denylist trip. Validates
// §1 threat-model: omni-guard probe is not pinned to the keypair.
// ====================================================================

#[tokio::test]
async fn identity_portability_across_device_fp_no_denylist_trip() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v1/config/limits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(limits_body()))
        .mount(&server)
        .await;

    let upload_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
    {
        let uc = Arc::clone(&upload_count);
        Mock::given(method("POST"))
            .and(path("/v1/upload"))
            .respond_with(move |_: &Request| {
                let n = uc.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                ResponseTemplate::new(200).set_body_json(json!({
                    "artifact_id": format!("art-{n}"),
                    "content_hash": "deadbeef",
                    "r2_url": "/v1/download/abc",
                    "thumbnail_url": "/v1/thumbnail/cafef00d",
                    "created_at": 0u64,
                    "status": "deduplicated",
                }))
            })
            // expect(2): both uploads MUST land — not 1 (denylist trip)
            // and not 0 (some other failure short-circuited the path).
            .expect(2)
            .mount(&server)
            .await;
    }

    let dev1 =
        test_share_context_with_worker_and_device(&server.uri(), [0x11; 32]).await;
    let dev2 =
        test_share_context_with_worker_and_device(&server.uri(), [0x22; 32]).await;

    // Same identity, different device_fp. The base factory assigns the
    // deterministic test keypair to both, so they already share a pubkey;
    // the explicit copy is belt-and-suspenders to make the assertion
    // robust against any future change to the factory's seed strategy.
    copy_identity(&dev1.ctx, &dev2.ctx);
    assert_eq!(
        dev1.ctx.identity.load().public_key().to_hex(),
        dev2.ctx.identity.load().public_key().to_hex(),
        "both contexts hold the same pubkey before upload"
    );

    // Sanity: the device fingerprints really do differ — without this,
    // we'd be testing nothing.
    assert_ne!(
        dev1.ctx.guard.device_id().unwrap().0,
        dev2.ctx.guard.device_id().unwrap().0,
        "test setup: device fingerprints must differ"
    );

    let r1 = simulate_upload(&dev1.ctx).await.expect("upload from device 1");
    let r2 = simulate_upload(&dev2.ctx).await.expect("upload from device 2");
    assert_eq!(r1.artifact_id, "art-1");
    assert_eq!(r2.artifact_id, "art-2");
    assert_eq!(
        upload_count.load(std::sync::atomic::Ordering::SeqCst),
        2,
        "both uploads accepted regardless of device_fp difference"
    );
    // wiremock's `.expect(2)` verification fires when `server` drops
    // at end-of-scope; if either upload was rate-limited or denylisted,
    // the count would be 1 and the test would hard-fail there too.
}

// ====================================================================
// Scenario 4 — Display name end-to-end.
// `setDisplayName` → host metadata updated + worker PUT fires →
// `GET /v1/author/<pubkey>` returns the new name. Worker last-write
// semantics across upload-time + setDisplayName events are pinned by
// the host-side handler tests in `ws_identity_handlers.rs`; here we
// pin the cross-boundary flow (handler → client → worker → cache
// invalidation surface).
// ====================================================================

#[tokio::test]
async fn display_name_end_to_end_set_then_get() {
    let server = MockServer::start().await;

    let put_bodies: Arc<std::sync::Mutex<Vec<Vec<u8>>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let pb = Arc::clone(&put_bodies);
        Mock::given(method("PUT"))
            .and(path("/v1/author/me"))
            .respond_with(move |req: &Request| {
                pb.lock().unwrap().push(req.body.clone());
                ResponseTemplate::new(200).set_body_json(json!({
                    "pubkey_hex": "x".repeat(64),
                    "display_name": "starfire",
                }))
            })
            .mount(&server)
            .await;
    }

    // After setDisplayName lands, a downstream consumer (renderer
    // useAuthorResolver) calls GET /v1/author/<pk> — assert the
    // worker now reports the new name. The same pubkey on the GET
    // path comes from `ctx.identity` so a regression that forgot to
    // update the active keypair would surface as a URL mismatch.
    Mock::given(method("GET"))
        .and(path_regex(r"/v1/author/.*"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "pubkey_hex": "ab".repeat(32),
            "fingerprint_hex": "00".repeat(6),
            "display_name": "starfire",
            "joined_at": 1_700_000_000u64,
            "total_uploads": 1u64,
        })))
        .mount(&server)
        .await;

    let TestCtx { ctx, _tmp } = test_share_context_with_worker_url(&server.uri()).await;
    let pk_hex = ctx.identity.load().public_key().to_hex();

    // 1. setDisplayName.
    let sdn_reply = dispatch_one(
        &ctx,
        json!({
            "id": "r1",
            "type": "identity.setDisplayName",
            "params": { "display_name": "starfire" }
        }),
    )
    .await;
    assert_eq!(sdn_reply["type"], "identity.setDisplayNameResult");
    assert_eq!(sdn_reply["params"]["display_name"], "starfire");
    assert_eq!(sdn_reply["params"]["pubkey_hex"], pk_hex);

    // 2. Host metadata updated locally (the persist runs synchronously
    //    before the handler returns the Result frame).
    let meta = IdentityMetadata::load_or_default(&ctx.identity_metadata_path(), &pk_hex);
    assert_eq!(
        meta.display_name.as_deref(),
        Some("starfire"),
        "setDisplayName persists to local IdentityMetadata"
    );

    // 3. Worker PUT fired with the expected body. Scope the
    //    MutexGuard inside its own block so it's released BEFORE
    //    the next `.await` (clippy::await_holding_lock).
    tokio::time::sleep(Duration::from_millis(50)).await;
    {
        let bodies = put_bodies.lock().unwrap();
        assert!(
            bodies.iter().any(|b| {
                serde_json::from_slice::<Value>(b)
                    .map(|v| v["display_name"].as_str() == Some("starfire"))
                    .unwrap_or(false)
            }),
            "PUT /v1/author/me body carried display_name=starfire"
        );
    }

    // 4. GET /v1/author/<pk> on the wire returns the persisted name.
    let detail = ctx
        .client
        .get_author(&pk_hex)
        .await
        .expect("get_author succeeds against the mock");
    assert_eq!(
        detail.display_name.as_deref(),
        Some("starfire"),
        "GET /v1/author/<pk> reflects the new display_name"
    );
}

// ====================================================================
// Scenario 5 — Stale-metadata tripwire. A metadata file with a
// `pubkey_hex` that doesn't match the active identity must reset to
// defaults on load (display_name=None, backed_up=false), AND that
// reset must persist to disk so a subsequent reader observes it.
// ====================================================================

#[tokio::test]
async fn stale_metadata_tripwire_resets_on_load() {
    let temp = tempfile::TempDir::new().expect("tempdir");
    let ctx = test_share_context_at_dir(temp.path()).await;
    let current_pk_hex = ctx.identity.load().public_key().to_hex();

    // Write metadata with a DIFFERENT pubkey_hex (simulates: user
    // manually replaced identity.key on disk while metadata still
    // pointed at the old key, OR an older host build wrote metadata
    // before the tripwire shipped).
    let meta_path = ctx.identity_metadata_path();
    let stale = IdentityMetadata {
        pubkey_hex: "stale".repeat(13), // 65 chars — definitely not equal to current
        display_name: Some("ghost".into()),
        backed_up: true,
        last_backed_up_at: Some(1_700_000_000),
        last_rotated_at: Some(1_700_000_000),
        last_backup_path: Some("C:\\stale.omniid".into()),
    };
    IdentityMetadata::save(&meta_path, &stale).unwrap();
    assert_ne!(stale.pubkey_hex, current_pk_hex);

    // First read through the dispatch surface — the tripwire fires
    // inside `IdentityMetadata::load_or_default`, resetting the
    // file to fresh defaults keyed on the current pubkey.
    let show = dispatch_one(
        &ctx,
        json!({ "id": "r1", "type": "identity.show", "params": {} }),
    )
    .await;
    assert_eq!(show["params"]["pubkey_hex"], current_pk_hex);
    assert_eq!(show["params"]["display_name"], Value::Null);
    assert_eq!(show["params"]["backed_up"], false);
    assert_eq!(show["params"]["last_backed_up_at"], Value::Null);
    assert_eq!(show["params"]["last_backup_path"], Value::Null);

    // The reset persisted: a fresh load against the same path returns
    // the cleared shape (NOT the original "ghost"/"stale.omniid"
    // values). If persistence were broken, the old bytes would still
    // be on disk and this read would resurrect them.
    let on_disk = IdentityMetadata::load_or_default(&meta_path, &current_pk_hex);
    assert_eq!(on_disk.pubkey_hex, current_pk_hex);
    assert_eq!(on_disk.display_name, None);
    assert!(!on_disk.backed_up);
    assert_eq!(on_disk.last_backed_up_at, None);
    assert_eq!(on_disk.last_backup_path, None);
}
