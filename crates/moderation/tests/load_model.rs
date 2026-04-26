//! End-to-end smoke for [`moderation::NudeNetModel`].
//!
//! Loads the bundled `apps/desktop/resources/moderation/nudenet.onnx` model
//! and runs it against a content-free fixture image to assert:
//!
//! 1. Model loads without error (download-binaries onnxruntime statically
//!    linked into the test binary works on this host).
//! 2. Inference returns a [`ModerationResult`] with `unsafe_score` below the
//!    INV-7.7.3 rejection threshold (0.8) for a benign image.
//!
//! These are integration-grade assertions per writing-lessons §E4: factory
//! constructed via the public API, full pipeline (load → preprocess →
//! `Session::run` → post-process → public result type) exercised.

use std::path::PathBuf;

use moderation::{ModerationResult, NudeNetModel};

/// Resolve the bundled NudeNet model path from the worktree root.
///
/// `CARGO_MANIFEST_DIR` is `crates/moderation/`; the model lives under
/// `apps/desktop/resources/moderation/` from the workspace root. Going up two
/// levels gets us back to the workspace root regardless of which worktree the
/// test is running in.
fn model_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root above crates/moderation/")
        .join("apps")
        .join("desktop")
        .join("resources")
        .join("moderation")
        .join("nudenet.onnx")
}

#[test]
fn loads_bundled_model() {
    let path = model_path();
    assert!(path.exists(), "fixture model missing at {}", path.display());
    NudeNetModel::load(&path).expect("model should load");
}

#[test]
fn benign_fixture_passes_threshold() {
    let path = model_path();
    let mut model = NudeNetModel::load(&path).expect("model should load");
    let bytes = include_bytes!("fixtures/clean-grey.png");
    let result: ModerationResult = model.check(bytes).expect("inference");
    assert!(
        result.unsafe_score < 0.8,
        "benign fixture flagged: score={} label={}",
        result.unsafe_score,
        result.label
    );
}
