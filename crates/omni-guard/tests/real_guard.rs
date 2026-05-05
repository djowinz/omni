//! Real-guard machine-reality tests. Equivalent to the private repo's
//! `tests/real_guard.rs` after the open-sourcing rename.

use omni_guard::{Guard, RealGuard};

#[test]
fn device_id_is_stable_and_32_bytes() {
    let g = RealGuard::new().expect("construct guard");
    let a = g.device_id().expect("device_id");
    let b = g.device_id().expect("device_id");
    assert_eq!(a, b);
    assert_eq!(a.0.len(), 32);
}

#[test]
fn device_id_is_non_zero_on_real_hardware() {
    let g = RealGuard::new().expect("construct guard");
    let id = g.device_id().expect("device_id");
    assert_ne!(
        id.0, [0u8; 32],
        "fingerprint collapsed to zeros — MAC/GUID/CPU all failed"
    );
}

#[test]
fn verify_self_integrity_ok_in_dev_build() {
    // No OMNI_GUARD_TEXT_SHA256 baked in, strict-integrity OFF → Ok.
    let g = RealGuard::new().expect("construct guard");
    g.verify_self_integrity().expect("integrity passes in dev build");
}

#[test]
fn is_vm_returns_without_panic() {
    let g = RealGuard::new().expect("construct guard");
    let _: bool = g.is_vm();
}

const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<RealGuard>();
};
