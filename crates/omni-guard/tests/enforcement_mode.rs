//! Pin the EnforcementMode contract so a future trait-method rename or
//! default-impl regression is caught immediately. Spec §4.

use omni_guard::{DisabledGuard, EnforcementMode, Guard, RealGuard};

#[test]
fn real_guard_returns_real_mode() {
    let g = RealGuard::new().expect("RealGuard::new() must succeed in test build");
    assert_eq!(g.enforcement_mode(), EnforcementMode::Real);
}

#[test]
fn disabled_guard_returns_disabled_mode() {
    assert_eq!(DisabledGuard.enforcement_mode(), EnforcementMode::Disabled);
}

#[test]
fn enforcement_mode_traits() {
    // Cheap derives — Copy, PartialEq — used by the host startup check.
    let real = EnforcementMode::Real;
    let disabled = EnforcementMode::Disabled;
    assert_ne!(real, disabled);
    let copied = real;
    assert_eq!(real, copied);
}

const _: fn() = || {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn Guard>();
};
