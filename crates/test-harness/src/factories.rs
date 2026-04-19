//! Production-shaped construction helpers. Factories in this module return
//! state wired exactly the way the `omni-host` startup path wires it, with
//! the stub guard and a deterministic identity key.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex};

use bundle::{BundleLimits, FileEntry, Manifest, Tag};
use host::share::cache::CachedArtifactDetail;
use host::share::client::ShareClient;
use host::share::preview::{PreviewSlot, ThemeSwap};
use host::share::registry::{RegistryHandle, RegistryKind};
use host::share::tofu::TofuStore;
use host::share::ws_messages::ShareContext;
use identity::Keypair;
use omni_guard_trait::{Guard, StubGuard};
use semver::Version;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use url::Url;

/// Inert theme-swap used by factories that don't exercise preview behavior.
struct InertThemeSwap;

impl ThemeSwap for InertThemeSwap {
    fn snapshot(&self) -> Vec<u8> {
        Vec::new()
    }
    fn apply(&self, _css: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn revert(&self, _snapshot: &[u8]) -> Result<(), String> {
        Ok(())
    }
}

/// Construct a [`ShareContext`] wired the way `omni-host` wires it at
/// startup, rooted at `data_dir` (pass a `tempfile::TempDir::path()` so the
/// caller owns the directory lifetime).
///
/// Uses [`deterministic_keypair`] for the identity, `StubGuard` for the
/// guard, and a local-only `wiremock`-compatible `ShareClient` base URL
/// (`http://127.0.0.1:0/`) that the caller replaces if it spins up a mock.
pub fn build_share_context(data_dir: &Path) -> ShareContext {
    let identity = Arc::new(deterministic_keypair());
    let guard: Arc<dyn Guard> = Arc::new(StubGuard);
    let base = Url::parse("http://127.0.0.1:0/").unwrap();
    let client = Arc::new(ShareClient::new(base, identity.clone(), guard.clone()));
    let tofu = Arc::new(Mutex::new(TofuStore::open(data_dir).expect("tofu open")));
    let bundles_registry = Arc::new(Mutex::new(
        RegistryHandle::load(data_dir, RegistryKind::Bundles).expect("bundles registry"),
    ));
    let themes_registry = Arc::new(Mutex::new(
        RegistryHandle::load(data_dir, RegistryKind::Themes).expect("themes registry"),
    ));
    let limits = Arc::new(Mutex::new(BundleLimits::DEFAULT));
    let current_version = Version::new(99, 0, 0);
    let preview_slot = Arc::new(PreviewSlot::new());
    let cancel_registry: Arc<Mutex<HashMap<String, CancellationToken>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let theme_swap: Arc<dyn ThemeSwap> = Arc::new(InertThemeSwap);
    let data_dir_buf = data_dir.to_path_buf();
    ShareContext {
        identity,
        guard,
        client,
        tofu,
        bundles_registry,
        themes_registry,
        limits,
        current_version,
        preview_slot,
        cancel_registry,
        theme_swap,
        data_dir: data_dir_buf,
    }
}

/// Deterministic identity key derived from a fixed seed. Same bytes every run
/// across machines — enables stable fingerprint / signature assertions.
///
/// Note: uses `Keypair::from_seed_for_test` (the public test-only constructor)
/// since `Keypair::from_seed` is `pub(crate)` and not accessible outside the
/// identity crate.
pub fn deterministic_keypair() -> Keypair {
    // Fixed 32-byte seed. Value chosen to be visually distinct from any
    // real fixture seed used in the worker test suite (0x07 repeated) —
    // see apps/worker/test/e2e_roundtrip.test.ts for the 0x07 seed.
    let seed = [0x42u8; 32];
    Keypair::from_seed_for_test(&seed)
}

/// Canonical `(Manifest, files)` tuple used across sanitize / bundle /
/// upload integration tests. Shape: one `overlay.omni` (real-format,
/// sourced from `crates/host/src/omni/assets/reference_overlay.omni`) plus
/// one `themes/default.css` theme. Deterministic — same bytes on every call.
pub fn marathon_fixture() -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let overlay_bytes = reference_overlay_source().to_vec();
    let theme_bytes = b":root { --bg: #000; --text: #fff; }\n".to_vec();
    let overlay_sha = sha256_of(&overlay_bytes);
    let theme_sha = sha256_of(&theme_bytes);

    let manifest = Manifest {
        schema_version: 1,
        name: "Marathon".into(),
        version: Version::new(1, 0, 0),
        omni_min_version: Version::new(0, 1, 0),
        description: "test-harness marathon fixture".into(),
        tags: vec![Tag::new("dark").unwrap()],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/default.css".into()),
        sensor_requirements: vec![],
        files: vec![
            FileEntry {
                path: "overlay.omni".into(),
                sha256: overlay_sha,
            },
            FileEntry {
                path: "themes/default.css".into(),
                sha256: theme_sha,
            },
        ],
        resource_kinds: None,
    };

    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), overlay_bytes);
    files.insert("themes/default.css".to_string(), theme_bytes);
    (manifest, files)
}

/// Packed + signed bundle bytes built from [`marathon_fixture`] using
/// [`deterministic_keypair`]. Suitable for any test that needs a real
/// `.omnipkg` blob (install, unpack, download roundtrip).
pub fn reference_overlay_bytes() -> Vec<u8> {
    let (manifest, files) = marathon_fixture();
    let kp = deterministic_keypair();
    identity::pack_signed_bundle(&manifest, &files, &kp, &BundleLimits::DEFAULT)
        .expect("pack reference bundle")
}

/// A populated sample list row matching the current worker `/v1/list` wire
/// shape. Typed against the Rust source that emits the TS interface — if
/// fields drift on either side, adding/removing them here fails compile.
pub fn sample_list_row() -> CachedArtifactDetail {
    CachedArtifactDetail {
        artifact_id: "sample-art-0001".into(),
        content_hash: "deadbeef".into(),
        author_pubkey: "0000000000000000000000000000000000000000000000000000000000000000".into(),
        name: "Sample".into(),
        kind: "theme".into(),
        r2_url: String::new(),
        thumbnail_url: "https://r2.test/thumb.png".into(),
        updated_at: 1_700_000_000,
        author_fingerprint_hex: String::new(),
        tags: vec!["dark".to_string()],
        installs: 0,
        created_at: 1_700_000_000,
    }
}

// ---- private helpers --------------------------------------------------------

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

fn reference_overlay_source() -> &'static [u8] {
    include_bytes!("../../host/src/omni/assets/reference_overlay.omni")
}

// ---- smoke tests ------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_share_context_populates_data_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = build_share_context(tmp.path());
        assert_eq!(ctx.data_dir, tmp.path());
        assert_eq!(ctx.current_version, Version::new(99, 0, 0));
    }

    #[test]
    fn deterministic_keypair_is_stable_across_calls() {
        let a = deterministic_keypair();
        let b = deterministic_keypair();
        assert_eq!(a.public_key().0, b.public_key().0);
    }

    #[test]
    fn marathon_fixture_has_overlay_and_theme() {
        let (manifest, files) = marathon_fixture();
        assert_eq!(manifest.name, "Marathon");
        assert!(files.contains_key("overlay.omni"));
        assert!(files.contains_key("themes/default.css"));
        assert_eq!(manifest.files.len(), 2);
    }

    #[test]
    fn reference_overlay_bytes_are_non_empty() {
        let bytes = reference_overlay_bytes();
        assert!(bytes.len() > 1000, "signed bundle should be substantial");
    }

    #[test]
    fn sample_list_row_populated() {
        let row = sample_list_row();
        assert!(!row.artifact_id.is_empty());
        assert!(!row.content_hash.is_empty());
        assert!(row.kind == "theme" || row.kind == "bundle");
    }
}
