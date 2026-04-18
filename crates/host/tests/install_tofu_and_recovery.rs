//! Integration tests for the install pipeline's TOFU warning path and the
//! atomic-dir crash-recovery sweep. Drives the library API directly (no
//! WebSocket); see plan Task 13 + 2026-04-14 amendment.

use std::collections::BTreeMap;

use bundle::{BundleLimits, FileEntry, Manifest, Tag};
use std::sync::Arc;

use identity::{pack_signed_bundle, Keypair};
use omni_guard_trait::{Guard, StubGuard};
use omni_host::share::client::ShareClient;
use omni_host::share::install::{install, InstallError, InstallRequest};
use omni_host::share::registry::{RegistryHandle, RegistryKind};
use omni_host::share::tofu::TofuStore;
use omni_host::workspace::atomic_dir::sweep_orphans;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Build a signed bundle whose manifest carries `name` and is signed by `kp`.
/// Two-file fixture: `overlay.omni` + `themes/theme.css` (mirrors install.rs).
fn build_signed_bundle(name: &str, kp: &Keypair) -> Vec<u8> {
    let overlay_bytes = b"<overlay></overlay>".to_vec();
    let theme_bytes = b"body { color: red; }".to_vec();
    let overlay_sha: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(&overlay_bytes);
        h.finalize().into()
    };
    let theme_sha: [u8; 32] = {
        let mut h = Sha256::new();
        h.update(&theme_bytes);
        h.finalize().into()
    };
    let manifest = Manifest {
        schema_version: 1,
        name: name.into(),
        version: semver::Version::new(1, 0, 0),
        omni_min_version: semver::Version::new(0, 1, 0),
        description: "fixture".into(),
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

#[tokio::test]
async fn second_pubkey_same_name_triggers_tofu_mismatch() {
    let kp_a = Keypair::generate();
    let kp_b = Keypair::generate();
    let bundle_a = build_signed_bundle("shared-name", &kp_a);
    let bundle_b = build_signed_bundle("shared-name", &kp_b);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/download/a"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bundle_a.clone()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/download/b"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bundle_b.clone()))
        .mount(&server)
        .await;

    let workspace = TempDir::new().unwrap();
    let app_data = TempDir::new().unwrap();

    let client = ShareClient::new(
        Url::parse(&server.uri()).unwrap(),
        Arc::new(Keypair::generate()),
        Arc::new(StubGuard) as Arc<dyn Guard>,
    );
    let mut tofu = TofuStore::open(app_data.path()).unwrap();
    let mut registry = RegistryHandle::load(app_data.path(), RegistryKind::Themes).unwrap();
    let limits = BundleLimits::DEFAULT;
    let current_version = semver::Version::new(99, 0, 0);

    // Install A — succeeds, records the (name -> pubkey_a) TOFU binding.
    let target_a = workspace.path().join("themes/a");
    let req_a = InstallRequest {
        artifact_id: "a".into(),
        target_path: target_a.clone(),
        overwrite: false,
        expected_pubkey: None,
    };
    let outcome_a = install(
        req_a,
        &client,
        &mut tofu,
        &mut registry,
        RegistryKind::Themes,
        &limits,
        &current_version,
        CancellationToken::new(),
        |_| {},
    )
    .await
    .expect("first install should succeed");
    assert!(target_a.exists(), "first install must materialize");
    assert_eq!(outcome_a.installed_path, target_a);

    // Install B — same display name, different signing key. TOFU rejects.
    let target_b = workspace.path().join("themes/b");
    let req_b = InstallRequest {
        artifact_id: "b".into(),
        target_path: target_b.clone(),
        overwrite: false,
        expected_pubkey: None,
    };
    let err = install(
        req_b,
        &client,
        &mut tofu,
        &mut registry,
        RegistryKind::Themes,
        &limits,
        &current_version,
        CancellationToken::new(),
        |_| {},
    )
    .await
    .expect_err("second install must fail with TOFU mismatch");
    match err {
        InstallError::TofuViolation { known, seen } => {
            assert_ne!(known, seen, "known and seen pubkeys must differ");
            assert!(!known.is_empty());
            assert!(!seen.is_empty());
        }
        other => panic!("expected TofuViolation, got: {other:?}"),
    }

    // Critical: TOFU rejection must happen before any filesystem work.
    assert!(
        !target_b.exists(),
        "second target dir must not be created on TOFU rejection"
    );
}

#[test]
fn sweep_orphans_removes_crash_leftovers() {
    let workspace = TempDir::new().unwrap();
    // Simulate a crashed mid-materialization: a staging dir with content.
    let staging = workspace.path().join(".omni-staging-aaa");
    std::fs::create_dir(&staging).unwrap();
    std::fs::write(staging.join("tmp.txt"), b"partial").unwrap();
    // Surviving sibling.
    let themes = workspace.path().join("themes");
    std::fs::create_dir(&themes).unwrap();

    let removed = sweep_orphans(workspace.path()).unwrap();
    assert_eq!(removed, 1, "should remove exactly one orphan");
    assert!(!staging.exists(), ".omni-staging-aaa must be gone");
    assert!(themes.exists(), "themes/ must survive sweep");
}
