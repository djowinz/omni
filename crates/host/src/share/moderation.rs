//! Host-side moderation wrapper around the `moderation` crate.
//!
//! Loads two complementary classifiers once on host startup and reuses them
//! for every Preview Image accept (INV-7.7.2 site #1) and every pack-time
//! Dependency Check image scan (INV-7.7.2 site #2):
//!
//! - **NudeNet** (`onnx-nudenet-v1`, threshold [`NUDENET_THRESHOLD`] = `0.4`):
//!   YOLO-style part detector. Catches isolated/pixelated explicit anatomy
//!   that whole-image classifiers miss because the composition doesn't match
//!   their porn-scene training data.
//! - **Falconsai NSFW** (`onnx-falconsai-vit-v1`, threshold
//!   [`FALCONSAI_THRESHOLD`] = `0.5`): ViT binary classifier. Catches
//!   "covered but suggestive" composition (bikini photos, suggestive game art)
//!   that part-detectors miss because no `EXPOSED_*` class fires.
//!
//! [`check_image`] runs both in parallel via `std::thread::scope` and rejects
//! if EITHER crosses its threshold (OR-logic). Each empirically caught a
//! distinct failure mode of the other; running both closes both gaps.
//!
//! ## Partial-init resilience
//!
//! Either model may be absent (dev contributors building without one of the
//! files staged). [`init_with_paths`] accepts `Option<PathBuf>` for each and
//! `check_image` runs whichever loaded successfully, degrading the gate
//! coverage rather than rejecting outright. If BOTH are absent, `check_image`
//! returns [`CheckError::NotInitialized`] and `main.rs` surfaces this to the
//! renderer as a structured admin error.
//!
//! ## Threshold + model history
//!
//! - **NudeNet 320n @ 0.8** (original): too lax — only photographic nudity.
//! - **NudeNet @ 0.4** (2026-04-27): caught the photographic anime test image
//!   but missed bikini-photo / Tifa / 2B class because covered anatomy never
//!   triggers `EXPOSED_*`.
//! - **Falconsai ViT @ 0.5** (2026-04-28 first half): caught covered/suggestive
//!   content but missed an isolated, partly-pixelated anatomical product shot
//!   (returned `safe / 0.0`) because that composition is outside its training
//!   distribution.
//! - **NudeNet @ 0.4 + Falconsai @ 0.5, OR-logic** (current, 2026-04-28): each
//!   covers the other's empirically-observed gap. Single-classifier solutions
//!   demonstrably leave both kinds of holes; two-model gate closes them.
//!
//! See spec §8.5 + INV-7.7.2 + INV-7.7.3.
//!
//! # Architectural invariant #24 — process-global mutable state
//!
//! Two cached singletons (`NUDENET_MODEL`, `FALCONSAI_MODEL`). Per
//! invariant #24:
//!
//! 1. **Ownership.** Both `Session`s are reachable from (a) the
//!    renderer-initiated `share.moderationCheck` WS handler and (b) the
//!    pack-time `share::dep_resolver`. Both call-sites enter through
//!    [`check_image`] which acquires both Mutexes for the inference window.
//! 2. **Concurrency.** Each model's `check` requires `&mut self`; per-model
//!    `Mutex` serializes its own queue. The two models do NOT block each
//!    other — `check_image` parallelizes via `std::thread::scope` so total
//!    latency is `max(nudenet, falconsai)` ≈ 80–200ms, not the sum.
//! 3. **Why singletons.** Combined ~96 MB of weights + ~500ms cold load.
//!    Threading two `Arc<Mutex<...>>` through every layer that doesn't need
//!    moderation would be invasive; both use sites are leaves at opposite
//!    ends of the host module graph.
//!
//! Recovery on poisoning is non-graceful: a panic during either `check`
//! poisons that model's `Mutex`, and subsequent calls return
//! [`CheckError::LockPoisoned`].

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::thread;

use ::moderation::{ModerationError, ModerationResult, NsfwClassifier, NudeNetModel};

/// Reject threshold for NudeNet (per-class detection confidence). See module
/// doc "Threshold + model history" for rationale.
pub const NUDENET_THRESHOLD: f32 = 0.4;
/// Reject threshold for Falconsai (whole-image NSFW probability). The
/// conventional argmax cutoff for the binary classifier; drop toward `~0.35`
/// for stricter "covered but suggestive" coverage if the default proves too
/// permissive.
pub const FALCONSAI_THRESHOLD: f32 = 0.5;

/// Detector identifier surfaced to the renderer's INV-7.7.6 detail strip.
pub const NUDENET_DETECTOR_ID: &str = "onnx-nudenet-v1";
/// Detector identifier surfaced to the renderer's INV-7.7.6 detail strip.
pub const FALCONSAI_DETECTOR_ID: &str = "onnx-falconsai-vit-v1";

/// Bundled NudeNet detector filename.
const NUDENET_FILENAME: &str = "nudenet.onnx";
/// Bundled Falconsai NSFW classifier filename.
const FALCONSAI_FILENAME: &str = "nsfw_falconsai.onnx";

static NUDENET_MODEL: OnceLock<Mutex<NudeNetModel>> = OnceLock::new();
static FALCONSAI_MODEL: OnceLock<Mutex<NsfwClassifier>> = OnceLock::new();

/// Outcome of a single moderation check at the host boundary.
///
/// `unsafe_score` and `label` come from the higher-scoring model when both
/// fire (or the only one that did when only one fires). `detector` is the
/// detector ID of the model whose result is reported, joined by `+` if both
/// rejected (e.g. `onnx-nudenet-v1+onnx-falconsai-vit-v1`).
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// Highest of the per-model scores in `[0.0, 1.0]`.
    pub unsafe_score: f32,
    /// Label from the model whose score is reported. NudeNet returns its
    /// triggering class (e.g. `FEMALE_BREAST_EXPOSED`); Falconsai returns
    /// `"nsfw"` or `"safe"`.
    pub label: String,
    /// Detector ID(s) that produced the reported result. Single ID when one
    /// model fired (or both passed); `"a+b"` when both rejected.
    pub detector: String,
    /// `true` if EITHER model crossed its threshold.
    pub rejected: bool,
}

/// Errors raised by the host moderation wrapper.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    /// Neither model is loaded. Call [`init_with_paths`] first; if the
    /// bundled files were missing at startup, the host's main loop logs a
    /// degraded-mode warning and the gate stays uninitialized — every check
    /// returns this error.
    #[error("no moderation model loaded; call init_with_paths first")]
    NotInitialized,

    /// Inner crate raised a load / inference error from one of the models.
    /// The first model error wins when both fail in the same call.
    #[error("inner moderation: {0}")]
    Inner(#[from] ModerationError),

    /// One of the model `Mutex`es was poisoned by a previous panic during
    /// `check`. The host can't recover without restarting (see module doc).
    #[error("model lock poisoned")]
    LockPoisoned,
}

/// Initialize the model singletons.
///
/// Each path is `Option<PathBuf>` — pass `None` to leave that model
/// uninitialized (e.g. the bundled file is missing in dev). Idempotent per
/// model: a second call with `Some(path)` for an already-initialized model is
/// a no-op.
///
/// Returns the first load error if any; the other model is still initialized
/// on best-effort. Callers should log per-model success separately rather
/// than treating partial init as a hard failure (see `main.rs`).
///
/// # Panics
///
/// `OnceLock::get_or_init` cannot return `Result`, so a failed load inside
/// the closure panics. That's the right shape for a host startup gate — if a
/// file is present but unloadable (corrupted, wrong ABI), the host can't
/// honor its INV-7.7.\* contract and should fail loudly.
pub fn init_with_paths(
    nudenet: Option<&Path>,
    falconsai: Option<&Path>,
) -> Result<(), ModerationError> {
    if let Some(path) = nudenet {
        let path = path.to_path_buf();
        NUDENET_MODEL.get_or_init(|| {
            Mutex::new(
                NudeNetModel::load(&path)
                    .unwrap_or_else(|err| panic!("nudenet load failed: {err}")),
            )
        });
    }
    if let Some(path) = falconsai {
        let path = path.to_path_buf();
        FALCONSAI_MODEL.get_or_init(|| {
            Mutex::new(
                NsfwClassifier::load(&path)
                    .unwrap_or_else(|err| panic!("falconsai classifier load failed: {err}")),
            )
        });
    }
    Ok(())
}

/// Per-model bundled-file location resolution.
///
/// Each field is `Some(path)` if the file exists in either the installed
/// (`<exe-dir>/resources/moderation/`) or dev
/// (`crates/moderation/resources/` relative to CWD) layout, `None`
/// otherwise. Callers feed this into [`init_with_paths`].
#[derive(Debug, Clone)]
pub struct ModelPaths {
    pub nudenet: Option<PathBuf>,
    pub falconsai: Option<PathBuf>,
}

/// Resolve the bundled model paths for the current process layout.
///
/// Tries each model's filename in (order):
/// 1. **Installed app:** `<exe-dir>/resources/moderation/<filename>`. Matches
///    electron-builder's `extraResources` layout — see
///    `apps/desktop/electron-builder.yml`, which mirrors the
///    crate-resources files into the install's `resources/moderation/` dir.
/// 2. **Dev (`cargo run` from the workspace root):**
///    `crates/moderation/resources/<filename>`. The models are owned by the
///    `moderation` crate (loaded by it, tested by it); `crates/host`
///    consumes them through the public API. Same crate-owned-resources
///    pattern as `crates/host/resources/feather.{ttf,css}`.
pub fn default_model_paths() -> ModelPaths {
    ModelPaths {
        nudenet: resolve_bundled(NUDENET_FILENAME),
        falconsai: resolve_bundled(FALCONSAI_FILENAME),
    }
}

fn resolve_bundled(filename: &str) -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let installed = dir.join("resources").join("moderation").join(filename);
            if installed.exists() {
                return Some(installed);
            }
        }
    }
    let dev = PathBuf::from("crates/moderation/resources").join(filename);
    if dev.exists() {
        Some(dev)
    } else {
        None
    }
}

/// Run moderation on a single image's raw bytes against both models in
/// parallel and OR-reduce the rejections.
///
/// Returns a [`CheckResult`] composed from per-model outputs. See [`CheckResult`]
/// docs for how scores/labels/detectors are reported in single-fire vs
/// both-fire vs both-pass cases.
pub fn check_image(bytes: &[u8]) -> Result<CheckResult, CheckError> {
    let nudenet = NUDENET_MODEL.get();
    let falconsai = FALCONSAI_MODEL.get();
    if nudenet.is_none() && falconsai.is_none() {
        return Err(CheckError::NotInitialized);
    }

    // Run both inferences concurrently. Each scoped thread acquires its own
    // model's Mutex so they don't contend with each other; total latency is
    // ~max(nudenet, falconsai) rather than the sum.
    let (nudenet_res, falconsai_res) = thread::scope(|s| {
        let n = nudenet.map(|m| s.spawn(move || run_check(m, bytes)));
        let f = falconsai.map(|m| s.spawn(move || run_check(m, bytes)));
        (
            n.map(|h| h.join().expect("nudenet inference thread panicked")),
            f.map(|h| h.join().expect("falconsai inference thread panicked")),
        )
    });

    let nudenet_out = nudenet_res.transpose()?;
    let falconsai_out = falconsai_res.transpose()?;

    let nudenet_rejected = nudenet_out
        .as_ref()
        .map(|r| r.unsafe_score >= NUDENET_THRESHOLD)
        .unwrap_or(false);
    let falconsai_rejected = falconsai_out
        .as_ref()
        .map(|r| r.unsafe_score >= FALCONSAI_THRESHOLD)
        .unwrap_or(false);

    Ok(reduce(
        nudenet_out,
        falconsai_out,
        nudenet_rejected,
        falconsai_rejected,
    ))
}

/// Generic inference helper — same shape works for both models because
/// [`NudeNetModel::check`] and [`NsfwClassifier::check`] both implement the
/// `Checker` trait below.
fn run_check<M: Checker>(model: &Mutex<M>, bytes: &[u8]) -> Result<ModerationResult, CheckError> {
    let mut guard = model.lock().map_err(|_| CheckError::LockPoisoned)?;
    guard.check_bytes(bytes).map_err(CheckError::Inner)
}

/// Trait erasing the difference between the two model types so [`run_check`]
/// can be parameterized once. Both crates' `check` methods already match this
/// signature; this trait just gives them a common name.
trait Checker {
    fn check_bytes(&mut self, bytes: &[u8]) -> Result<ModerationResult, ModerationError>;
}

impl Checker for NudeNetModel {
    fn check_bytes(&mut self, bytes: &[u8]) -> Result<ModerationResult, ModerationError> {
        self.check(bytes)
    }
}

impl Checker for NsfwClassifier {
    fn check_bytes(&mut self, bytes: &[u8]) -> Result<ModerationResult, ModerationError> {
        self.check(bytes)
    }
}

/// Compose the two per-model results into one [`CheckResult`].
///
/// Reporting policy:
/// - **Both rejected:** report the higher-scoring result; `detector` is
///   `"<higher>+<lower>"` (preserves both IDs for diagnostics, higher first).
/// - **One rejected:** report that model's result; `detector` is its ID.
/// - **Neither rejected:** report the higher-scoring of the two safe results
///   so the dev-mode chrome shows the highest sub-threshold signal; detector
///   is the higher one's ID.
fn reduce(
    nudenet: Option<ModerationResult>,
    falconsai: Option<ModerationResult>,
    nudenet_rejected: bool,
    falconsai_rejected: bool,
) -> CheckResult {
    // Tag each result with its detector id so we don't have to keep track of
    // which option is which by position downstream.
    let n = nudenet.map(|r| (r, NUDENET_DETECTOR_ID));
    let f = falconsai.map(|r| (r, FALCONSAI_DETECTOR_ID));

    match (n, f, nudenet_rejected, falconsai_rejected) {
        (Some((nr, n_id)), Some((fr, f_id)), true, true) => {
            // Both rejected — report the higher score; detector is
            // `"<higher>+<lower>"` so the chrome can show both fired.
            let (high, low_id) = if nr.unsafe_score >= fr.unsafe_score {
                ((nr, n_id), f_id)
            } else {
                ((fr, f_id), n_id)
            };
            CheckResult {
                unsafe_score: high.0.unsafe_score,
                label: high.0.label,
                detector: format!("{}+{}", high.1, low_id),
                rejected: true,
            }
        }
        (Some((nr, n_id)), _, true, false) => CheckResult {
            unsafe_score: nr.unsafe_score,
            label: nr.label,
            detector: n_id.to_string(),
            rejected: true,
        },
        (_, Some((fr, f_id)), false, true) => CheckResult {
            unsafe_score: fr.unsafe_score,
            label: fr.label,
            detector: f_id.to_string(),
            rejected: true,
        },
        // Neither rejected — pick the higher safe score for diagnostic chrome.
        (Some((nr, n_id)), Some((fr, f_id)), false, false) => {
            let (winner, id) = if nr.unsafe_score >= fr.unsafe_score {
                (nr, n_id)
            } else {
                (fr, f_id)
            };
            CheckResult {
                unsafe_score: winner.unsafe_score,
                label: winner.label,
                detector: id.to_string(),
                rejected: false,
            }
        }
        (Some((nr, n_id)), None, false, false) => CheckResult {
            unsafe_score: nr.unsafe_score,
            label: nr.label,
            detector: n_id.to_string(),
            rejected: false,
        },
        (None, Some((fr, f_id)), false, false) => CheckResult {
            unsafe_score: fr.unsafe_score,
            label: fr.label,
            detector: f_id.to_string(),
            rejected: false,
        },
        // Both `None` is filtered out at the top of `check_image` via
        // `NotInitialized`; defensively map to safe with the falconsai ID.
        (None, None, _, _) => CheckResult {
            unsafe_score: 0.0,
            label: "safe".to_string(),
            detector: FALCONSAI_DETECTOR_ID.to_string(),
            rejected: false,
        },
        // Rust can't prove that `xxx_rejected: true` implies the matching
        // option is `Some`, but `check_image` computes both flags from the
        // option-and-score: a `None` option always yields `unwrap_or(false)`
        // → false. The remaining `(None, _, true, _)` etc. arms are
        // structurally unreachable.
        _ => unreachable!(
            "rejected flag is true but the corresponding model option is None — \
             invariant violated upstream of reduce()"
        ),
    }
}
