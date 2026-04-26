//! Host-side moderation wrapper around the `moderation` crate.
//!
//! Loads the bundled NudeNet ONNX detector once on host startup and reuses
//! the loaded `Session` for every Preview Image accept (INV-7.7.2 site #1) and
//! every pack-time Dependency Check image scan (INV-7.7.2 site #2). The
//! wrapper applies the INV-7.7.3 rejection threshold (`0.8`); the inner crate
//! still returns the raw `(unsafe_score, label)` so future tooling can inspect
//! sub-threshold detections.
//!
//! See spec §8.5 + INV-7.7.2 + INV-7.7.3.
//!
//! # Architectural invariant #24 — process-global mutable state justification
//!
//! The cached singleton (`static MODEL: OnceLock<Mutex<NudeNetModel>>`) is
//! intentional. Per invariant #24, additions of process-global mutable state
//! require explicit documentation:
//!
//! 1. **Ownership diagram.** The `Session` is reachable from two host call
//!    paths: (a) the renderer-initiated `share.moderationCheck` WS handler
//!    (Wave B1; runs on the `tokio` runtime that drives the WS server), and
//!    (b) the pack-time `share::dep_resolver` (Wave B1; runs on whichever
//!    `spawn_blocking` task drives the pack pipeline). Both paths take the
//!    `Mutex` for the duration of one inference and release it.
//!
//! 2. **Concurrency contract.** `NudeNetModel::check` takes `&mut self` because
//!    `ort::Session::run` requires exclusive access. The `Mutex` serializes
//!    overlapping inference calls. Single-image inference is ~50–150 ms on CPU
//!    for the 320×320 detector; the upload pipeline never has more than a
//!    handful of inflight checks (one Preview + N≤5 bundled images), so
//!    contention is bounded. No fairness, no priority — first lock wins.
//!
//! 3. **Why explicit state passing doesn't work.** The `Session` is ~12 MB of
//!    ONNX weights plus the native `onnxruntime` library handle; cold load is
//!    ~200 ms. Threading it through the WS server, file watcher, dep resolver,
//!    pack pipeline, and renderer-RPC handler would push a `Arc<Mutex<...>>`
//!    through every layer that doesn't otherwise need to know moderation
//!    exists. The two call sites are on opposite ends of the host's module
//!    graph (WS handler vs. pack pipeline) and have no shared call-stack
//!    ancestor short of `main.rs`. The singleton concentrates the handle at
//!    its leaf use-sites; explicit threading would leak it through ~6
//!    intermediate signatures.
//!
//! Recovery on poisoning is non-graceful: a panic during `check` poisons the
//! `Mutex`, and subsequent calls return `CheckError::LockPoisoned`. Practical
//! impact is small — if `Session::run` panicked, the bundled model is broken
//! and the host needs a restart to pick up a re-bundled binary anyway.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use ::moderation::{ModerationError, ModerationResult, NudeNetModel};

/// Confidence threshold above which an image is rejected by the upload
/// pipeline (INV-7.7.3).
pub const REJECTION_THRESHOLD: f32 = 0.8;

/// Cached NudeNet model. First call to [`init_with_path`] initializes; every
/// subsequent [`check_image`] reuses through the `Mutex`. See module-level
/// invariant #24 justification for why the singleton is acceptable.
static MODEL: OnceLock<Mutex<NudeNetModel>> = OnceLock::new();

/// Outcome of a single moderation check at the host boundary.
///
/// Carries the raw inner score + label (so logs / dev-mode chrome can render
/// INV-7.7.6's `code Moderation:ClientRejected · detector onnx-nudenet-v1 ·
/// confidence 0.XX`) plus a precomputed `rejected` boolean derived from
/// [`REJECTION_THRESHOLD`].
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Maximum unsafe-class confidence from the inner detector (range
    /// `[0.0, 1.0]`).
    pub unsafe_score: f32,
    /// Triggering class label, or `"safe"` if no unsafe-class detection
    /// crossed the inner detection floor.
    pub label: String,
    /// `unsafe_score >= REJECTION_THRESHOLD`.
    pub rejected: bool,
}

/// Errors raised by the host moderation wrapper.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    /// `check_image` was called before `init_with_path`. Call sites must
    /// initialize the singleton during host startup before exposing the
    /// `share.moderationCheck` WS endpoint or running pack-time dependency
    /// checks.
    #[error("model not yet loaded; call init_with_path first")]
    NotInitialized,

    /// Inner crate raised a load / inference error. Carries the original
    /// [`ModerationError`] for diagnostic logging.
    #[error("inner moderation: {0}")]
    Inner(#[from] ModerationError),

    /// The cached `Mutex` was poisoned by a previous panic during `check`.
    /// The host can't recover without restarting (see module doc).
    #[error("model lock poisoned")]
    LockPoisoned,
}

/// Initialize the singleton with a model file path.
///
/// Idempotent — only the first call performs the load; subsequent calls
/// return `Ok(())` without re-initializing. Intended to be called once from
/// the host startup path with the resolved bundled-model location.
///
/// # Behavior on load failure
///
/// `OnceLock::get_or_init` cannot return `Result`, so a failed load inside
/// the closure currently panics. That's the right shape for a host startup
/// gate — if the bundled model is missing or corrupted the host can't honor
/// its INV-7.7.\* contract and should fail loudly. Callers that want to
/// pre-validate the path can [`std::path::Path::exists`] check before
/// calling. Once the host startup wiring lands (Wave B1) we can revisit and
/// route the load failure into a structured startup error.
pub fn init_with_path(path: impl Into<PathBuf>) -> Result<(), ModerationError> {
    let path: PathBuf = path.into();
    MODEL.get_or_init(|| {
        Mutex::new(
            NudeNetModel::load(&path)
                .unwrap_or_else(|err| panic!("nudenet load failed: {err}")),
        )
    });
    Ok(())
}

/// Resolve the bundled NudeNet model path for the current process layout.
///
/// Tries (in order):
/// 1. **Installed app:** `<exe-dir>/resources/moderation/nudenet.onnx`. This
///    matches the `extraResources` layout produced by electron-builder
///    (`apps/desktop/electron-builder.yml` Wave B0.1 entry).
/// 2. **Dev (`cargo run` from the workspace root):**
///    `apps/desktop/resources/moderation/nudenet.onnx`. CWD is the workspace
///    root in the standard `cargo run -p host` flow.
///
/// Returns `None` if neither location holds the model — the host startup
/// path can then surface a structured error instead of bubbling up a
/// `Mutex<...>` lock failure later.
pub fn default_model_path() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let installed = dir
                .join("resources")
                .join("moderation")
                .join("nudenet.onnx");
            if installed.exists() {
                return Some(installed);
            }
        }
    }
    let dev = PathBuf::from("apps/desktop/resources/moderation/nudenet.onnx");
    if dev.exists() {
        Some(dev)
    } else {
        None
    }
}

/// Run moderation on a single image's raw bytes.
///
/// Returns a [`CheckResult`] with the inner detector's `(score, label)` plus
/// a `rejected` flag from comparing the score against [`REJECTION_THRESHOLD`].
/// Errors:
/// - [`CheckError::NotInitialized`] — call [`init_with_path`] first.
/// - [`CheckError::Inner`] — inner moderation crate failure (bad image,
///   unexpected output tensor shape, etc.).
/// - [`CheckError::LockPoisoned`] — a previous call panicked; restart the host.
pub fn check_image(bytes: &[u8]) -> Result<CheckResult, CheckError> {
    let model = MODEL.get().ok_or(CheckError::NotInitialized)?;
    let mut guard = model.lock().map_err(|_| CheckError::LockPoisoned)?;
    let inner: ModerationResult = guard.check(bytes)?;
    Ok(CheckResult {
        rejected: inner.unsafe_score >= REJECTION_THRESHOLD,
        unsafe_score: inner.unsafe_score,
        label: inner.label,
    })
}
