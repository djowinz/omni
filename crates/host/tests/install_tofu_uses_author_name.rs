//! Per 2026-04-26 identity-completion-and-display-name spec §3.5: TOFU stores
//! the AUTHOR's `display_name` (resolved from worker via `GET /v1/author/<pk>`),
//! NOT the bundle's `manifest.name`.
//!
//! Pre-spec bug (#015 carryover): `install.rs` passed `signed.manifest().name`
//! as the TOFU `display_name` slot, conflating the artifact label with the
//! author identity label. This regression test exercises both paths through
//! the public `install` library API:
//!
//! - **Worker returns AuthorDetail with display_name**: TOFU stores the
//!   worker-resolved name (e.g. `"starfire"`), NOT the bundle name (e.g.
//!   `"cool-overlay"`).
//! - **Worker offline / 404 / network failure**: `get_author().ok().and_then(...)`
//!   collapses to `None`; TOFU stores `display_name: None`. Better to label
//!   nothing than to label wrong — downstream UIs render the pubkey-slice
//!   when the label is absent.

use std::collections::BTreeMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use bundle::{BundleLimits, FileEntry, Manifest, Tag};
use identity::{pack_signed_bundle, Keypair, TofuRegistry};
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

/// Build a signed bundle whose `manifest.name` is `bundle_name`. The signing
/// keypair (the AUTHOR identity) is returned alongside so the caller can
/// pre-mount a `GET /v1/author/<pk>` handler keyed on the matching pubkey.
fn build_signed_bundle(bundle_name: &str) -> (Vec<u8>, Keypair, BundleLimits) {
    let kp = Keypair::generate();
    let overlay_bytes = b"<widget><template><div/></template></widget>".to_vec();
    let theme_bytes = b"body { color: red; }".to_vec();
    let overlay_sha = sha256_of(&overlay_bytes);
    let theme_sha = sha256_of(&theme_bytes);

    let manifest = Manifest {
        schema_version: 1,
        name: bundle_name.to_string(),
        version: semver::Version::new(1, 0, 0),
        omni_min_version: semver::Version::new(0, 1, 0),
        description: "T11 fixture".into(),
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

    let limits = BundleLimits::DEFAULT;
    let bytes = pack_signed_bundle(&manifest, &files, &kp, &limits).unwrap();
    (bytes, kp, limits)
}

/// Per spec §3.5 happy path: when the worker resolves the author's
/// `display_name` to `"starfire"`, TOFU records `"starfire"` — NOT
/// the bundle's `manifest.name` (`"cool-overlay"`).
#[tokio::test]
async fn tofu_records_author_display_name_not_bundle_name() {
    let (bundle_bytes, author_kp, limits) = build_signed_bundle("cool-overlay");
    let author_pubkey = author_kp.public_key();
    let author_pubkey_hex = author_pubkey.to_hex();
    let artifact_id = "art-starfire-001";

    // Wiremock serves both:
    //  - GET /v1/download/<id> → bundle bytes
    //  - GET /v1/author/<pubkey_hex> → AuthorDetail with display_name="starfire"
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/download/{artifact_id}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(bundle_bytes.clone())
                .insert_header("content-type", "application/octet-stream"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/author/{author_pubkey_hex}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "pubkey_hex": author_pubkey_hex,
                "fingerprint_hex": author_pubkey.fingerprint().to_hex(),
                "display_name": "starfire",
                "joined_at": 0,
                "total_uploads": 1,
            })),
        )
        .mount(&server)
        .await;

    let workspace = TempDir::new().unwrap();
    let app_data = TempDir::new().unwrap();

    // The ShareClient's own signing identity is unrelated to the bundle's
    // author identity (the host installing IS NOT the bundle's author).
    let client = ShareClient::new(
        Url::parse(&server.uri()).unwrap(),
        Arc::new(ArcSwap::new(Arc::new(Keypair::generate()))),
        Arc::new(StubGuard) as Arc<dyn Guard>,
    );
    let mut tofu = TofuStore::open(app_data.path()).unwrap();
    let mut registry = RegistryHandle::load(app_data.path(), RegistryKind::Themes).unwrap();

    let target_path = workspace.path().join("themes").join("cool-overlay");
    let req = InstallRequest {
        artifact_id: artifact_id.into(),
        target_path: target_path.clone(),
        overwrite: false,
        expected_pubkey: None,
    };

    let _outcome = install(
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

    // Reload the on-disk TOFU registry and assert the author entry's
    // `display_name` is the worker-resolved one — `"starfire"` — NOT the
    // bundle's `manifest.name` `"cool-overlay"`.
    let tofu_path = app_data.path().join("tofu-fingerprints.json");
    let reloaded = TofuRegistry::load(&tofu_path).expect("tofu reload");
    let entry = reloaded
        .entry(author_pubkey)
        .expect("first-seen TOFU entry recorded for the bundle's author");
    assert_eq!(
        entry.display_name.as_deref(),
        Some("starfire"),
        "TOFU must store the AUTHOR's worker-resolved display_name, not the bundle name"
    );
    assert_ne!(
        entry.display_name.as_deref(),
        Some("cool-overlay"),
        "regression guard: bundle name must NEVER appear in TOFU's display_name slot"
    );
}

/// Per spec §3.5 offline path: when the worker is unreachable (no handler
/// mounted → 404 / connection error), `get_author().ok()` collapses to
/// `None` and TOFU stores `display_name: None`. Pins the "no label > wrong
/// label" invariant.
#[tokio::test]
async fn tofu_records_none_when_worker_offline() {
    let (bundle_bytes, author_kp, limits) = build_signed_bundle("another-overlay");
    let author_pubkey = author_kp.public_key();
    let artifact_id = "art-no-author-002";

    // Wiremock serves the download but NOT the author endpoint — every
    // GET /v1/author/<pk> falls through wiremock's default 404. The install
    // path must treat that as "no label available" and store None.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/download/{artifact_id}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(bundle_bytes.clone())
                .insert_header("content-type", "application/octet-stream"),
        )
        .mount(&server)
        .await;

    let workspace = TempDir::new().unwrap();
    let app_data = TempDir::new().unwrap();

    let client = ShareClient::new(
        Url::parse(&server.uri()).unwrap(),
        Arc::new(ArcSwap::new(Arc::new(Keypair::generate()))),
        Arc::new(StubGuard) as Arc<dyn Guard>,
    );
    let mut tofu = TofuStore::open(app_data.path()).unwrap();
    let mut registry = RegistryHandle::load(app_data.path(), RegistryKind::Themes).unwrap();

    let target_path = workspace.path().join("themes").join("another-overlay");
    let req = InstallRequest {
        artifact_id: artifact_id.into(),
        target_path: target_path.clone(),
        overwrite: false,
        expected_pubkey: None,
    };

    let _outcome = install(
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
    .expect("install must succeed even when the author endpoint 404s");

    // Reload TOFU and assert the entry exists with display_name = None.
    // No label > wrong label — downstream UIs render the pubkey-slice
    // disambiguator when display_name is absent.
    let tofu_path = app_data.path().join("tofu-fingerprints.json");
    let reloaded = TofuRegistry::load(&tofu_path).expect("tofu reload");
    let entry = reloaded
        .entry(author_pubkey)
        .expect("first-seen TOFU entry recorded for the bundle's author");
    assert_eq!(
        entry.display_name, None,
        "worker offline → TOFU stores None (no label is better than a wrong label)"
    );
    assert_ne!(
        entry.display_name.as_deref(),
        Some("another-overlay"),
        "regression guard: bundle name must NEVER appear in TOFU's display_name slot"
    );
}
