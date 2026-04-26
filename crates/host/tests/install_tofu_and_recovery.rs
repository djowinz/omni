//! Integration tests for the install pipeline's atomic-dir crash-recovery
//! sweep. Drives the library API directly (no WebSocket); see plan Task 13 +
//! 2026-04-14 amendment.
//!
//! The previous `second_pubkey_same_name_triggers_tofu_mismatch` test was
//! removed in T1 of the 2026-04-26 identity-completion-and-display-name
//! plan: the `TofuResult::DisplayNameMismatch` variant + impersonation check
//! it exercised were dropped (display_names are non-unique under the
//! `<display_name>#<8-hex>` disambiguation scheme).

use tempfile::TempDir;

use omni_host::workspace::atomic_dir::sweep_orphans;

#[test]
fn sweep_orphans_removes_crash_leftovers() {
    let workspace = TempDir::new().unwrap();
    // Simulate a crashed mid-materialization: a staging dir with content.
    let staging = workspace.path().join(".omni-staging-aaa");
    std::fs::create_dir(&staging).unwrap();
    std::fs::write(staging.join("tmp.txt"), b"partial").unwrap();
    // Surviving sibling.
    let themes = workspace.path().join("themes");
    std::fs::create_dir(&themes).unwrap();

    let removed = sweep_orphans(workspace.path()).unwrap();
    assert_eq!(removed, 1, "should remove exactly one orphan");
    assert!(!staging.exists(), ".omni-staging-aaa must be gone");
    assert!(themes.exists(), "themes/ must survive sweep");
}
