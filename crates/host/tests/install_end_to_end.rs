//! Sub-spec 010 §8.2 umbrella end-to-end integration test.
//!
//! Drives the full Worker -> host -> workspace install path:
//!   1. Pack a signed bundle in-process via `identity::pack_signed_bundle`.
//!   2. Serve the bytes from a `wiremock::MockServer` on `GET /v1/download/:id`.
//!   3. Call the public `omni_host::share::install::install` library API end-to-end.
//!   4. Assert the workspace contents, registry state, and absence of staging
//!      residue.
//!
//! Per the 2026-04-14 amendment, sub-spec 010's WS wiring is deferred to a
//! post-Phase-2 async-bridge chore, so this test exercises the library API
//! directly rather than going through a WebSocket client.

use std::collections::BTreeMap;

use bundle::{BundleLimits, FileEntry, Manifest, Tag};
use std::sync::Arc;

use identity::{pack_signed_bundle, Keypair};
use omni_guard_trait::{Guard, StubGuard};
use omni_host::share::client::ShareClient;
use omni_host::share::install::{install, InstallRequest};
use omni_host::share::registry::{RegistryHandle, RegistryKind};
use omni_host::share::tofu::TofuStore;
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

#[tokio::test]
async fn worker_to_host_to_workspace_roundtrip() {
    // ---- 1. Build a signed bundle (mirrors install.rs::tests::build_fixture) ----
    let kp = Keypair::generate();
    let overlay_bytes = b"<overlay></overlay>".to_vec();
    let theme_bytes = b"body { color: red; }".to_vec();
    let overlay_sha = sha256_of(&overlay_bytes);
    let theme_sha = sha256_of(&theme_bytes);

    let manifest = Manifest {
        schema_version: 1,
        name: "e2e-theme".into(),
        version: semver::Version::new(1, 0, 0),
        omni_min_version: semver::Version::new(0, 1, 0),
        description: "umbrella §8.2 e2e fixture".into(),
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
    files.insert("overlay.omni".to_string(), overlay_bytes.clone());
    files.insert("themes/theme.css".to_string(), theme_bytes.clone());

    let limits = BundleLimits::DEFAULT;
    let bundle_bytes = pack_signed_bundle(&manifest, &files, &kp, &limits).unwrap();
    let expected_content_hash = hex::encode(sha256_of(&bundle_bytes));

    // ---- 2. Stand up wiremock with the bundle bytes ------------------------
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/download/abc123"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(bundle_bytes.clone())
                .insert_header("content-type", "application/octet-stream"),
        )
        .mount(&server)
        .await;

    // ---- 3. Workspace + app-data temp dirs ---------------------------------
    let workspace = TempDir::new().unwrap();
    let app_data = TempDir::new().unwrap();

    // ---- 4. Construct the host-side dependencies ---------------------------
    let client = ShareClient::new(
        Url::parse(&server.uri()).unwrap(),
        Arc::new(Keypair::generate()),
        Arc::new(StubGuard) as Arc<dyn Guard>,
    );
    let mut tofu = TofuStore::open(app_data.path()).unwrap();
    let mut registry = RegistryHandle::load(app_data.path(), RegistryKind::Themes).unwrap();

    // ---- 5. Build the install request --------------------------------------
    let target_path = workspace.path().join("themes").join("e2e-theme");
    let req = InstallRequest {
        artifact_id: "abc123".into(),
        target_path: target_path.clone(),
        overwrite: false,
        expected_pubkey: None,
    };

    // ---- 6. Drive the install end-to-end -----------------------------------
    let outcome = install(
        req,
        &client,
        &mut tofu,
        &mut registry,
        RegistryKind::Themes,
        &limits,
        &semver::Version::new(99, 0, 0),
        CancellationToken::new(),
        |_p| {},
    )
    .await
    .expect("install must succeed end-to-end");

    // ---- 7a. Workspace contents written under preserved subdir layout ------
    assert_eq!(outcome.installed_path, target_path);
    let theme_file = outcome.installed_path.join("themes").join("theme.css");
    assert!(
        theme_file.exists(),
        "themes/theme.css must exist after install"
    );
    let theme_on_disk = std::fs::read(&theme_file).unwrap();
    assert_eq!(
        theme_on_disk, theme_bytes,
        "theme.css contents match fixture"
    );

    let overlay_file = outcome.installed_path.join("overlay.omni");
    assert!(
        overlay_file.exists(),
        "overlay.omni must exist after install"
    );
    let overlay_on_disk = std::fs::read(&overlay_file).unwrap();
    assert_eq!(
        overlay_on_disk, overlay_bytes,
        "overlay.omni contents match fixture"
    );

    // ---- 7b. Registry round-trip from disk: exactly one entry, matches ------
    drop(registry); // ensure save() landed; reload from disk fresh
    let reloaded = RegistryHandle::load(app_data.path(), RegistryKind::Themes).unwrap();
    let entries = reloaded.entries();
    assert_eq!(entries.len(), 1, "exactly one installed-themes entry");
    let entry = entries
        .get("e2e-theme")
        .expect("entry keyed by display name");
    assert_eq!(entry.content_hash, expected_content_hash);
    assert_eq!(entry.installed_version, semver::Version::new(1, 0, 0));
    assert_eq!(entry.artifact_id, "abc123");

    // ---- 7c. No staging residue under the parent install dir ---------------
    let parent = workspace.path().join("themes");
    let staging_leftovers = std::fs::read_dir(&parent)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.starts_with(".omni-staging-"))
                .unwrap_or(false)
        })
        .count();
    assert_eq!(staging_leftovers, 0, "no .omni-staging-* residue");
}
