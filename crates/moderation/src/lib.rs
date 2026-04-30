//! Local NSFW moderation for Omni's upload pipeline.
//!
//! Two complementary classifiers run in parallel via the host wrapper
//! (`crates/host/src/share/moderation.rs`):
//!
//! - **NudeNet** ([`NudeNetModel`]) — YOLO-style part detector for explicit
//!   anatomy (`EXPOSED_*` classes). Catches isolated/pixelated explicit
//!   content that whole-image classifiers miss because the composition
//!   doesn't match porn-scene training data.
//! - **Falconsai NSFW** ([`NsfwClassifier`]) — ViT binary classifier for
//!   whole-image NSFW probability. Catches "covered but suggestive"
//!   composition (bikini photos, suggestive game art) that part-detectors
//!   miss because no `EXPOSED_*` class fires.
//!
//! The host wrapper reduces both with OR-logic — if EITHER model crosses its
//! threshold, the image is rejected. See `host::share::moderation` for the
//! threshold values and the gate composition.
//!
//! Crate boundary stays format/inference only — caller (host) is responsible
//! for: routing decisions, logging, surfacing rejection chrome, and threshold
//! comparisons. See spec §8.5.
//!
//! ## Shared result + error types
//!
//! Both classifiers produce the same `(unsafe_score: f32, label: String)`
//! shape; differences live in what the label means (NudeNet → unsafe-class
//! name, Falconsai → `"nsfw"`/`"safe"`). Defined here at crate root rather
//! than per-module so the host wrapper can compare them with one type and so
//! we don't have two near-identical `ModerationError` enums.

pub mod nsfw_classifier;
pub mod nudenet;

pub use nsfw_classifier::NsfwClassifier;
pub use nudenet::NudeNetModel;

/// Result of a single moderation check from either classifier.
///
/// `unsafe_score` is in `[0.0, 1.0]` — its precise semantics are
/// classifier-specific:
/// - **NudeNet:** maximum per-anchor confidence across the unsafe-class set.
/// - **Falconsai:** softmaxed `nsfw` class probability.
///
/// `label` is `"safe"` for the no-signal case across both classifiers; on
/// trigger NudeNet returns the unsafe class name (e.g. `FEMALE_BREAST_EXPOSED`)
/// and Falconsai returns `"nsfw"`.
#[derive(Debug, Clone, PartialEq)]
pub struct ModerationResult {
    pub unsafe_score: f32,
    pub label: String,
}

impl ModerationResult {
    /// Convenience constructor for the "no unsafe signal" case.
    pub fn safe() -> Self {
        Self {
            unsafe_score: 0.0,
            label: "safe".to_string(),
        }
    }
}

/// Errors raised by either classifier's load/inference path.
#[derive(Debug, thiserror::Error)]
pub enum ModerationError {
    /// ORT failure (model load, session run, tensor extraction).
    #[error("ort: {0}")]
    Ort(#[from] ort::Error),

    /// Image decode/format failure.
    #[error("image decode: {0}")]
    Image(#[from] image::ImageError),

    /// File I/O failure (reading model from disk).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Output tensor had an unexpected shape — usually means the bundled
    /// model file isn't the format the wrapper expects. `expected` describes
    /// the shape pattern we wanted; `got` is the actual shape from the model.
    #[error("unexpected output tensor shape: expected {expected}, got {got:?}")]
    UnexpectedShape {
        expected: &'static str,
        got: Vec<usize>,
    },
}
