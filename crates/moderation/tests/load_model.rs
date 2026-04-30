//! End-to-end smoke for [`moderation::NsfwClassifier`].
//!
//! Loads the bundled `crates/moderation/resources/nsfw_falconsai.onnx` model
//! (owned by this crate; the desktop installer mirrors it into its install
//! resources at packaging time via `apps/desktop/electron-builder.yml`) and
//! runs it against a content-free fixture image to assert:
//!
//! 1. Model loads without error (download-binaries onnxruntime statically
//!    linked into the test binary works on this host).
//! 2. Inference returns a [`ModerationResult`] with `unsafe_score` below the
//!    INV-7.7.3 rejection threshold (currently `0.5` — see
//!    `host::share::moderation::REJECTION_THRESHOLD`) for a benign image.
//!    The constant lives in the `host` crate which this crate doesn't depend
//!    on, so the value is replicated here. Keep the two in sync if the gate
//!    is re-tuned.
//!
//! These are integration-grade assertions per writing-lessons §E4: factory
//! constructed via the public API, full pipeline (load → preprocess →
//! `Session::run` → post-process → public result type) exercised.

use std::path::PathBuf;

use moderation::{ModerationResult, NsfwClassifier};

/// Resolve the bundled NSFW classifier model path inside this crate's
/// `resources/` directory. `CARGO_MANIFEST_DIR` is `crates/moderation/`, so
/// the path resolves regardless of which worktree the test runs in.
fn model_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("nsfw_falconsai.onnx")
}

#[test]
fn loads_bundled_model() {
    let path = model_path();
    assert!(path.exists(), "fixture model missing at {}", path.display());
    NsfwClassifier::load(&path).expect("model should load");
}

#[test]
fn benign_fixture_passes_threshold() {
    let path = model_path();
    let mut model = NsfwClassifier::load(&path).expect("model should load");
    let bytes = include_bytes!("fixtures/clean-grey.png");
    let result: ModerationResult = model.check(bytes).expect("inference");
    assert!(
        result.unsafe_score < 0.5,
        "benign fixture flagged at-or-above the host gate: score={} label={}",
        result.unsafe_score,
        result.label
    );
}
