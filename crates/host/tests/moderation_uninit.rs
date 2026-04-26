//! Companion to `moderation_onnx.rs` — exercises the `NotInitialized` error
//! path in its own test binary so the singleton from the other suite doesn't
//! taint the assertion.
//!
//! Cargo gives every `tests/*.rs` file its own binary, so the `OnceLock` in
//! `omni_host::share::moderation` starts uninitialized here regardless of
//! sibling test execution order.

use omni_host::share::moderation::{check_image, CheckError};

const ONE_PIXEL_PNG: &[u8] = include_bytes!(
    "../../moderation/tests/fixtures/clean-pixel.png"
);

#[test]
fn check_image_before_init_returns_not_initialized() {
    let err = check_image(ONE_PIXEL_PNG).expect_err("must fail before init");
    assert!(
        matches!(err, CheckError::NotInitialized),
        "expected NotInitialized, got {err:?}"
    );
}
