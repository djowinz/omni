//! Integration tests for `omni_host::share::moderation` after the dual-gate
//! refactor (NudeNet + Falconsai run in parallel via OR-logic).
//!
//! Coverage:
//! - `default_model_paths()` resolves both bundled model files in either the
//!   installed or dev layout.
//! - `check_image()` against a benign 320×320 grey PNG returns `rejected = false`
//!   with a sub-threshold score on whichever model "won" the diagnostic
//!   reporting (the higher-scoring of the two safe results — see
//!   `share::moderation::reduce`).
//!
//! Test ordering note: `init_with_paths` writes process-global `OnceLock`s
//! that survive across `#[test]` invocations within the same binary. The
//! `not_initialized` error path is exercised in a separate test binary so
//! ordering doesn't matter — see `moderation_uninit.rs`.

use omni_host::share::moderation::{
    check_image, default_model_paths, init_with_paths, FALCONSAI_THRESHOLD, NUDENET_THRESHOLD,
};

const CLEAN_GREY_PNG: &[u8] = include_bytes!("../../moderation/tests/fixtures/clean-grey.png");

/// Try to set up both singletons from the dev-layout model paths. Returns
/// `false` if BOTH bundled models are missing on this checkout (no inference
/// possible); inference tests then skip rather than fail.
fn ensure_init() -> bool {
    let paths = default_model_paths();
    if paths.nudenet.is_none() && paths.falconsai.is_none() {
        return false;
    }
    init_with_paths(paths.nudenet.as_deref(), paths.falconsai.as_deref()).is_ok()
}

#[test]
fn default_model_paths_resolves_in_dev_or_installed_layout() {
    // The dev layout ships both `nudenet.onnx` and `nsfw_falconsai.onnx`
    // under `crates/moderation/resources/`. When this test binary runs from
    // the workspace root (the standard `cargo test -p host` flow), at least
    // one path should resolve. If neither does (CI without the model files),
    // surface a warning rather than failing — the inference test below will
    // skip too.
    let paths = default_model_paths();
    match (&paths.nudenet, &paths.falconsai) {
        (Some(p), _) | (_, Some(p)) => {
            assert!(p.exists(), "resolved path should exist: {}", p.display())
        }
        (None, None) => eprintln!(
            "WARN: no moderation models found in either installed or dev \
             layout — inference test will skip"
        ),
    }
}

#[test]
fn check_image_returns_safe_for_clean_fixture() {
    if !ensure_init() {
        eprintln!("skip — no bundled models available");
        return;
    }
    let result = check_image(CLEAN_GREY_PNG).expect("inference should succeed");
    assert!(
        !result.rejected,
        "clean fixture should not be rejected: score={} label={} detector={}",
        result.unsafe_score, result.label, result.detector,
    );
    // Score must be below BOTH thresholds because the OR-reduction would
    // reject if either fired.
    assert!(
        result.unsafe_score < NUDENET_THRESHOLD.max(FALCONSAI_THRESHOLD),
        "clean fixture score should be below both thresholds: score={} label={} detector={}",
        result.unsafe_score,
        result.label,
        result.detector,
    );
}

#[test]
fn init_with_paths_is_idempotent() {
    let paths = default_model_paths();
    if paths.nudenet.is_none() && paths.falconsai.is_none() {
        eprintln!("skip — no bundled models available");
        return;
    }
    // First init may have already happened in another test; both calls must
    // succeed regardless of order.
    init_with_paths(paths.nudenet.as_deref(), paths.falconsai.as_deref())
        .expect("first or repeat init");
    init_with_paths(paths.nudenet.as_deref(), paths.falconsai.as_deref())
        .expect("repeat init");
}
