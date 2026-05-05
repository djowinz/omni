//! Negative integrity test. Compiles only with `--features strict-integrity`
//! per `required-features` in Cargo.toml. CI invokes with a wrong
//! `OMNI_GUARD_TEXT_SHA256` env at build time; RealGuard::new() must fail.

use omni_guard::{GuardError, RealGuard};

#[test]
fn real_guard_construction_fails_with_wrong_expected_hash() {
    let result = RealGuard::new();
    match result {
        Err(GuardError::IntegrityFailed) => (),
        Err(other) => panic!("expected IntegrityFailed, got {other:?}"),
        Ok(_) => panic!("construction must fail when expected hash is wrong"),
    }
}
