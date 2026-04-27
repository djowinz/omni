//! Direct contract-crate tests for `Keypair::import_encrypted_and_write`.
//!
//! Per writing-lessons §E4: every public method on the identity crate
//! must have a direct test in `crates/identity/tests/`. This method was
//! added as an integration-discovered deviation (architectural invariant
//! #23) during T10 of the 2026-04-26 identity-completion-and-display-name
//! plan — host integration coverage exists in
//! `crates/host/tests/ws_identity_handlers.rs::import_swaps_active_keypair_*`,
//! but the identity crate itself owns the atomic-write + ACL semantics
//! and must exercise them directly.
//!
//! Coverage:
//!   - roundtrip persistence (export ⇒ import_and_write ⇒ load_or_create)
//!   - parent-dir creation for nested target paths
//!   - atomic-write semantics: bad passphrase leaves an existing on-disk
//!     file untouched (the failed import never reaches the rename step)

use identity::Keypair;
use tempfile::tempdir;

#[test]
fn import_encrypted_and_write_persists_correct_key_to_disk() {
    let kp = Keypair::generate();
    let blob = kp.export_encrypted("very-long-passphrase").unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("nested").join("identity.key");

    let recovered =
        Keypair::import_encrypted_and_write(&blob, "very-long-passphrase", &path).unwrap();
    assert_eq!(recovered.public_key(), kp.public_key());
    assert!(path.exists(), "identity.key written to disk");

    // Reload from disk via the standard loader → same keypair.
    let reloaded = Keypair::load_or_create(&path).unwrap();
    assert_eq!(reloaded.public_key(), kp.public_key());
}

#[test]
fn import_encrypted_and_write_creates_parent_dir_if_missing() {
    let kp = Keypair::generate();
    let blob = kp.export_encrypted("pw-pw-pw-pw-pw-pw-pw-pw").unwrap();
    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("a")
        .join("b")
        .join("c")
        .join("identity.key");

    let _kp =
        Keypair::import_encrypted_and_write(&blob, "pw-pw-pw-pw-pw-pw-pw-pw", &path).unwrap();
    assert!(path.exists(), "parent dirs were created");
}

#[test]
fn import_encrypted_and_write_leaves_existing_file_intact_on_bad_passphrase() {
    let kp = Keypair::generate();
    let blob = kp
        .export_encrypted("right-pass-right-pass-right-pass")
        .unwrap();
    let dir = tempdir().unwrap();
    let path = dir.path().join("identity.key");
    std::fs::write(&path, b"existing-bytes").unwrap();

    let result =
        Keypair::import_encrypted_and_write(&blob, "wrong-pass-wrong-pass-wrong-pass", &path);
    assert!(result.is_err(), "wrong passphrase must fail");

    // File unchanged: atomic_write writes-then-renames; failed import
    // never reaches the rename, so the existing bytes survive verbatim.
    assert_eq!(std::fs::read(&path).unwrap(), b"existing-bytes");
}
