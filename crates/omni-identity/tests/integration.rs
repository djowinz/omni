//! End-to-end integration tests for omni-identity.

use omni_identity::{Keypair, TofuRegistry, TofuResult};
use tempfile::tempdir;

#[test]
fn full_lifecycle_generate_backup_restore() {
    let dir = tempdir().unwrap();
    let key_path = dir.path().join("identity.key");

    let kp = Keypair::load_or_create(&key_path).unwrap();
    let pk = kp.public_key();
    let fp = kp.fingerprint();

    // Backup + restore via a different Keypair instance
    let backup = kp.export_encrypted("very-long-passphrase").unwrap();
    let restored = Keypair::import_encrypted(&backup, "very-long-passphrase").unwrap();
    assert_eq!(restored.public_key(), pk);
    assert_eq!(restored.fingerprint(), fp);

    // Reload from disk -> same pubkey
    let reloaded = Keypair::load_or_create(&key_path).unwrap();
    assert_eq!(reloaded.public_key(), pk);
}

#[test]
fn tofu_flags_impersonation_across_restart() {
    let dir = tempdir().unwrap();
    let tofu_path = dir.path().join("tofu.json");

    let kp_a = Keypair::generate();
    let kp_b = Keypair::generate();

    {
        let mut r = TofuRegistry::load(&tofu_path).unwrap();
        assert_eq!(
            r.check_or_record(kp_a.public_key(), "lx92", 1),
            TofuResult::FirstSeen
        );
        r.save().unwrap();
    }

    // New process: B claims the same display name.
    {
        let mut r = TofuRegistry::load(&tofu_path).unwrap();
        match r.check_or_record(kp_b.public_key(), "lx92", 2) {
            TofuResult::DisplayNameMismatch { .. } => {}
            other => panic!("expected mismatch, got {other:?}"),
        }
    }
}

#[test]
fn fingerprint_display_is_stable() {
    let kp = Keypair::generate();
    let s1 = kp.fingerprint().to_string();
    let s2 = kp.fingerprint().to_string();
    assert_eq!(s1, s2);
    assert_eq!(s1.matches('-').count(), 2);
}
