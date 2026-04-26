//! Integration tests for `omni_host::share::moderation` (Task B0.3 wrapper
//! around the `moderation` crate from B0.1).
//!
//! Coverage:
//! - `default_model_path()` resolves to one of the two supported install
//!   layouts when run from the workspace root.
//! - `check_image()` against the same benign 320×320 grey PNG fixture used by
//!   `crates/moderation/tests/load_model.rs` returns `rejected = false` and a
//!   sub-threshold (< INV-7.7.3 = 0.8) score.
//!
//! Test ordering note: `init_with_path` writes a process-global `OnceLock`
//! that survives across `#[test]` invocations within the same binary. The
//! `not_initialized` error path is exercised in a separate test binary so
//! ordering doesn't matter — see `moderation_uninit.rs`.

use omni_host::share::moderation::{check_image, default_model_path, init_with_path};

const CLEAN_GREY_PNG: &[u8] = include_bytes!(
    "../../moderation/tests/fixtures/clean-grey.png"
);

/// Try to set up the singleton from the dev-layout model path. Returns
/// `false` if the bundled model isn't present on this checkout (e.g. CI
/// runner without LFS / asset download); inference tests then skip rather
/// than fail.
fn ensure_init() -> bool {
    let Some(path) = default_model_path() else {
        return false;
    };
    init_with_path(path).is_ok()
}

#[test]
fn default_model_path_resolves_in_dev_or_installed_layout() {
    // The dev layout ships `apps/desktop/resources/moderation/nudenet.onnx`
    // as part of OWI-49's commit (`2749967`). When this test binary runs from
    // the workspace root (the standard `cargo test -p host` flow), the dev
    // path should resolve. If it doesn't (CI without the model file), surface
    // a warning rather than failing — the inference test below will skip too.
    match default_model_path() {
        Some(p) => assert!(p.exists(), "resolved path should exist: {}", p.display()),
        None => eprintln!(
            "WARN: nudenet.onnx not found in either installed or dev layout — \
             inference test will skip"
        ),
    }
}

#[test]
fn check_image_returns_safe_score_for_clean_fixture() {
    if !ensure_init() {
        eprintln!("skip — bundled model not available");
        return;
    }
    let result = check_image(CLEAN_GREY_PNG).expect("inference should succeed");
    assert!(
        !result.rejected,
        "clean fixture should not be rejected: score={} label={}",
        result.unsafe_score, result.label
    );
    assert!(
        result.unsafe_score < 0.8,
        "clean fixture score should be below INV-7.7.3 threshold: score={} label={}",
        result.unsafe_score, result.label
    );
}

#[test]
fn init_with_path_is_idempotent() {
    let Some(path) = default_model_path() else {
        eprintln!("skip — bundled model not available");
        return;
    };
    // First init may have already happened in another test; both calls must
    // succeed regardless of order.
    init_with_path(&path).expect("first or repeat init");
    init_with_path(&path).expect("repeat init");
}
