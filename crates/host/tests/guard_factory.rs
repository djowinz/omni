//! Host factory tests. Contract-level coverage for the Guard trait lives in
//! `omni-guard-trait/tests/contract.rs` (retro D-004-H). These tests confirm
//! only that `make_guard()` wires correctly in each feature mode.

#[test]
fn make_guard_returns_ok_and_valid_device_id() {
    let g = omni_host::guard::make_guard().expect("make_guard must succeed");
    let id = g.device_id().expect("device_id must succeed");
    assert_eq!(id.0.len(), 32);
}
