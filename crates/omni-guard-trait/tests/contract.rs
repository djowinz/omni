//! Trait-contract and type-derive tests for omni-guard-trait.
//! Per retro/2026-04-13-theme-sharing-004-design-retro.md D-004-H: contract
//! coverage lives with the contract, not in consumer crates.

use omni_guard_trait::{DeviceId, Guard, StubGuard};

// Compile-time assertion that Box<dyn Guard> is Send + Sync.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync + ?Sized>() {}
    assert_send_sync::<dyn Guard>();
};

#[test]
fn stub_guard_device_id_is_stable() {
    let g = StubGuard;
    let a = g.device_id().unwrap();
    let b = g.device_id().unwrap();
    assert_eq!(a, b);
    assert_eq!(a.0.len(), 32);
}

#[test]
fn stub_guard_verify_self_integrity_ok() {
    let g = StubGuard;
    g.verify_self_integrity().expect("stub integrity must pass");
}

#[test]
fn stub_guard_is_not_vm() {
    let g = StubGuard;
    assert!(!g.is_vm());
}

#[test]
fn device_id_hex_display_is_64_lowercase_chars() {
    let id = DeviceId([0xab; 32]);
    let s = format!("{id}");
    assert_eq!(s.len(), 64);
    assert!(s.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_eq!(s, "ab".repeat(32));
}

#[test]
fn device_id_equality_and_hash() {
    use std::collections::HashSet;
    let a = DeviceId([0x01; 32]);
    let b = DeviceId([0x01; 32]);
    let c = DeviceId([0x02; 32]);
    assert_eq!(a, b);
    assert_ne!(a, c);
    let mut set = HashSet::new();
    set.insert(a);
    assert!(set.contains(&b));
    assert!(!set.contains(&c));
}
