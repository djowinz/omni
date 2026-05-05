//! Pin make_guard()'s behavior under --features dev-no-guard. Compiles
//! only with the feature enabled (otherwise DisabledGuard isn't exported).
//!
//! The release startup gate in main.rs depends on this contract: when
//! the dev-no-guard feature flips make_guard to return DisabledGuard,
//! enforcement_mode() must return Disabled so the gate fires.

#![cfg(feature = "dev-no-guard")]

use omni_guard::EnforcementMode;

#[test]
fn make_guard_returns_disabled_under_dev_no_guard() {
    let g = omni_host::guard::make_guard().expect("make_guard ok");
    assert_eq!(
        g.enforcement_mode(),
        EnforcementMode::Disabled,
        "with --features dev-no-guard, make_guard MUST return a guard \
         whose enforcement_mode is Disabled (the release startup gate \
         depends on this)"
    );
}
