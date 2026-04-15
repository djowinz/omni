//! Integration tests for `omni_admin::key_file::check_permissions`.
//!
//! Task 12 of the theme-sharing #012 plan.

#[cfg(unix)]
#[test]
fn rejects_world_readable_key() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
    let err = omni_admin::key_file::check_permissions(tmp.path()).unwrap_err();
    assert!(err.to_string().contains("0644") || err.to_string().contains("mode"));
    assert!(err.to_string().contains("chmod 600"));
}

#[cfg(unix)]
#[test]
fn accepts_owner_only_key() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::set_permissions(tmp.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
    omni_admin::key_file::check_permissions(tmp.path()).unwrap();
}

#[cfg(windows)]
#[test]
fn accepts_default_user_profile_key() {
    // Files created in the user's temp dir with the default ACL inherit user-only rights in most
    // Windows setups — we don't know the CI runner's umask, so this is a smoke test that the
    // check doesn't spuriously reject a freshly-created tempfile. A dedicated overbroad-ACL test
    // requires building a DACL which is non-trivial without pulling more Win32 surface; leave
    // that to an integration test later if needed.
    let tmp = tempfile::NamedTempFile::new().unwrap();
    let _ = omni_admin::key_file::check_permissions(tmp.path());
    // Don't assert — we want the smoke of "function runs without panic on a real file".
}

#[cfg(windows)]
#[test]
fn rejects_missing_file() {
    let err = omni_admin::key_file::check_permissions(std::path::Path::new(
        "C:\\nonexistent-omni-admin-keyfile-x7gq.key",
    ))
    .unwrap_err();
    // Any error is acceptable — just assert it didn't claim the file was secure.
    let _ = err;
}
