//! Tests for `Keypair::generate_and_write` — the rotation primitive used by
//! the host's `identity.rotate` WS handler. Per 2026-04-26
//! identity-completion-and-display-name spec §2.

use identity::Keypair;
use tempfile::tempdir;

#[test]
fn generate_and_write_produces_new_keypair_replacing_existing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("identity.key");

    let kp1 = Keypair::generate_and_write(&path).unwrap();
    let pk1 = kp1.public_key();

    let kp2 = Keypair::generate_and_write(&path).unwrap();
    let pk2 = kp2.public_key();

    assert_ne!(pk1, pk2, "rotation must produce a different pubkey");

    let loaded = Keypair::load_or_create(&path).unwrap();
    assert_eq!(
        loaded.public_key(),
        pk2,
        "on-disk file reflects post-rotation key"
    );
}

#[test]
fn generate_and_write_creates_parent_dir_if_missing() {
    let dir = tempdir().unwrap();
    let path = dir
        .path()
        .join("nested")
        .join("subdir")
        .join("identity.key");

    let _kp = Keypair::generate_and_write(&path).unwrap();
    assert!(path.exists(), "parent dir was created");
}
