//! Shared test factories for `crates/host/tests/`.
//!
//! Per writing-lessons §D7: integration tests construct shared state via
//! a single factory path so production wiring and test wiring stay
//! shape-identical. The base [`build_share_context`] from `test-harness`
//! already covers the standard wiring; the helpers here layer on the
//! variants that the `identity_e2e.rs` scenarios need (custom worker
//! URL, custom device fingerprint, full upload roundtrip).
//!
//! Discipline:
//! - `tempfile::TempDir` per test — no shared state across tests.
//! - The [`TestCtx`] return value owns the tempdir alongside the
//!   `ShareContext`, so the dir lives at least as long as the context.
//!   Drop-order (ShareContext first, tempdir second) handles cleanup
//!   without races against any background tasks the handlers spawn.
//! - For wiremock variants, the caller mounts mocks BEFORE calling the
//!   factory so the resulting `ShareContext` already points at a fully-
//!   configured server.

#![allow(dead_code)] // not every helper is used by every test file in `tests/`

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use bundle::{FileEntry, Manifest, Tag};
use omni_guard_trait::{DeviceId, Guard, GuardError, StubGuard};
use omni_host::share::client::ShareClient;
use omni_host::share::upload::{PackResult, UploadResult};
use omni_host::share::ws_messages::ShareContext;
use semver::Version;
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use url::Url;

/// Bundle of [`ShareContext`] + the `TempDir` it was rooted at, so
/// tests can keep both alive together. Drop order: `ctx` first (calls
/// any sync teardown), then `_tmp` removes the directory.
///
/// We intentionally re-export the inner fields by deref-style methods
/// instead of `Deref<Target=ShareContext>` so tests calling
/// `ctx.identity.load()` keep their existing call shape.
pub struct TestCtx {
    pub ctx: ShareContext,
    pub _tmp: tempfile::TempDir,
}

/// Build a [`ShareContext`] rooted at a fresh tempdir. No worker
/// configured — the embedded `ShareClient` points at
/// `http://127.0.0.1:0/` per the `test-harness` default. Use this for
/// scenarios that don't make outbound HTTP calls.
pub async fn test_share_context_at_dir(dir: &std::path::Path) -> ShareContext {
    test_harness::build_share_context(dir)
}

/// Build a [`TestCtx`] with a fresh tempdir AND a `ShareClient`
/// rebuilt against `worker_url`. The keypair `Arc<ArcSwap<Keypair>>`
/// is preserved across the rebuild so a `.store(...)` from the test
/// retargets both `ctx.identity` and the embedded client signer in
/// lockstep — same shape as production wiring.
///
/// `worker_url` should be the bare wiremock origin (e.g.
/// `http://127.0.0.1:NNNN`); the trailing slash is appended here so
/// `Url::join` resolves `/v1/...` paths correctly.
pub async fn test_share_context_with_worker_url(worker_url: &str) -> TestCtx {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let mut ctx = test_harness::build_share_context(tmp.path());
    let url = Url::parse(&format!("{}/", worker_url.trim_end_matches('/')))
        .expect("worker_url parses to Url");
    ctx.client = Arc::new(ShareClient::new(
        url,
        ctx.identity.clone(),
        ctx.guard.clone(),
    ));
    TestCtx { ctx, _tmp: tmp }
}

/// Identical to [`test_share_context_with_worker_url`] but installs a
/// `FixedDeviceGuard` whose `device_id()` returns `device_bytes`. Used
/// by the identity-portability scenario to prove that two contexts
/// with the SAME pubkey but DIFFERENT device fingerprints both succeed
/// against the worker — `omni-guard` is probe-only and never pinned to
/// the keypair (spec §1 threat-model).
pub async fn test_share_context_with_worker_and_device(
    worker_url: &str,
    device_bytes: [u8; 32],
) -> TestCtx {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    // Build the base context, then swap in a FixedDeviceGuard wired
    // through the same `Arc<ArcSwap<Keypair>>` slot the base context
    // produced (so the keypair is shared with the rebuilt client).
    let mut ctx = test_harness::build_share_context(tmp.path());
    let guard: Arc<dyn Guard> = Arc::new(FixedDeviceGuard {
        device_bytes,
    });
    ctx.guard = guard.clone();
    let url = Url::parse(&format!("{}/", worker_url.trim_end_matches('/')))
        .expect("worker_url parses to Url");
    ctx.client = Arc::new(ShareClient::new(url, ctx.identity.clone(), guard));
    TestCtx { ctx, _tmp: tmp }
}

/// Copy the active keypair from `src` into `dst.identity` so two
/// contexts (with different device-fp guards) share the same Ed25519
/// keypair. Used by the portability scenario.
pub fn copy_identity(src: &ShareContext, dst: &ShareContext) {
    let kp_arc = src.identity.load_full();
    dst.identity.store(kp_arc);
}

/// Drive a single message through the public `dispatch` entry point
/// and return the synchronous reply frame parsed as JSON. Mirrors the
/// shape used in `ws_identity_handlers.rs` so test code reads the same
/// way across the suite.
pub async fn dispatch_one(
    ctx: &ShareContext,
    msg: serde_json::Value,
) -> serde_json::Value {
    let send_fn = move |_s: String| {};
    let reply = omni_host::share::ws_messages::dispatch(ctx, &msg, send_fn)
        .await
        .expect("identity handler returns a synchronous reply frame");
    serde_json::from_str(&reply).expect("reply is valid JSON")
}

/// Synthetic upload helper — exercises [`ShareClient::upload`] against
/// a wiremock server WITHOUT going through the production
/// `pack_only` Ultralight render path (which requires native
/// resources unsuitable for headless CI). Builds a deterministic
/// [`PackResult`] with a tiny manifest + a few placeholder bytes for
/// `sanitized_bytes` and `thumbnail_png`. The wire body is what the
/// test cares about (multipart POST hits the mock); the bytes
/// themselves are inert.
///
/// Returns the [`UploadResult`] from the (mocked) worker so the
/// caller can assert on `artifact_id` / `status`.
pub async fn simulate_upload(
    ctx: &ShareContext,
) -> Result<UploadResult, omni_host::share::error::UploadError> {
    let pack = synthetic_pack();
    let (tx, _rx) = mpsc::channel(8);
    ctx.client.upload(pack, tx).await
}

fn synthetic_pack() -> PackResult {
    // Deterministic minimal "theme" pack — the worker is mocked so the
    // bytes don't have to be a real .omnipkg; only the multipart POST
    // boundary needs to land. The fields below mirror what
    // `pack_only` produces in production so future drift in
    // `ShareClient::upload`'s consumption surfaces here too.
    let theme_bytes = b"body { color: red; }".to_vec();
    let theme_sha = sha256_of(&theme_bytes);

    let manifest = Manifest {
        schema_version: 1,
        name: "synthetic-theme".into(),
        version: Version::new(1, 0, 0),
        omni_min_version: Version::new(0, 1, 0),
        description: "synthetic e2e fixture".into(),
        tags: vec![Tag::new("dark").unwrap()],
        license: "MIT".into(),
        entry_overlay: "theme.css".into(),
        default_theme: None,
        sensor_requirements: vec![],
        files: vec![FileEntry {
            path: "theme.css".into(),
            sha256: theme_sha,
        }],
        resource_kinds: None,
    };

    let mut files = BTreeMap::new();
    files.insert("theme.css".to_string(), theme_bytes);

    // Hand-rolled placeholder for sanitized_bytes — the wiremock
    // doesn't validate the .omnipkg shape so we ship inert bytes.
    let sanitized_bytes = b"INERT_OMNIPKG_FOR_E2E_TEST".to_vec();
    let content_hash = hex::encode(sha256_of(&sanitized_bytes));
    // Tiniest valid PNG: 1x1 transparent pixel from the libpng test set.
    let thumbnail_png = tiny_png();

    PackResult {
        manifest,
        manifest_name: "synthetic-theme".into(),
        manifest_kind: "theme".into(),
        sanitized_bytes,
        content_hash,
        thumbnail_png,
        compressed_size: 0,
        uncompressed_size: 20,
        sanitize_report: serde_json::json!({}),
    }
}

fn tiny_png() -> Vec<u8> {
    // Smallest valid PNG (8-byte sig + IHDR + IDAT + IEND), 1x1 RGBA
    // transparent pixel. Worker accepts on byte length only.
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG sig
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR len + tag
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
        0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, // bit depth/CRC
        0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, // IDAT
        0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01,
        0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, // IEND
        0xAE, 0x42, 0x60, 0x82,
    ]
}

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

// ----------------------------------------------------------------------
// FixedDeviceGuard — Guard impl with caller-controlled device_id bytes.
// Used by the identity-portability scenario to exhibit two distinct
// device fingerprints from the same identity. Passes `verify_self_integrity`
// and `is_vm == false` so it doesn't trigger any other guard-driven path.
// ----------------------------------------------------------------------

pub struct FixedDeviceGuard {
    pub device_bytes: [u8; 32],
}

impl Guard for FixedDeviceGuard {
    fn device_id(&self) -> Result<DeviceId, GuardError> {
        Ok(DeviceId(self.device_bytes))
    }
    fn verify_self_integrity(&self) -> Result<(), GuardError> {
        Ok(())
    }
    fn is_vm(&self) -> bool {
        false
    }
}

// Compile-time pin: keep StubGuard reachable so editors / IDEs that
// strip unused imports don't quietly drop our cross-crate dependency
// edge — the `omni-guard-trait` crate is what re-exports Guard.
#[allow(dead_code)]
fn _stub_guard_pin() -> Arc<dyn Guard> {
    Arc::new(StubGuard)
}

// ----------------------------------------------------------------------
// Path helpers (re-exported via `pub use common::*` in the test files
// for ergonomic access).
// ----------------------------------------------------------------------

pub fn data_dir(ctx: &ShareContext) -> PathBuf {
    ctx.data_dir.clone()
}
