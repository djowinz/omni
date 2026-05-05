//! `make_guard()` wires correctly under the default + dev-no-guard features.
//! Trait-contract coverage lives in `crates/omni-guard/tests/contract.rs`.

#[test]
fn make_guard_returns_ok_and_valid_device_id() {
    let g = omni_host::guard::make_guard().expect("make_guard must succeed");
    let id = g.device_id().expect("device_id must succeed");
    assert_eq!(id.0.len(), 32);
}
