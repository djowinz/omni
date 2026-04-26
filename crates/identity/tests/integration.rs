//! End-to-end integration tests for omni-identity.

use identity::{Keypair, TofuRegistry, TofuResult};
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
fn export_import_roundtrip_survives_intervening_rotation() {
    // Per 2026-04-26 identity-completion spec §2: backup blobs must be
    // independent of on-disk state. After exporting kp1, rotating the
    // on-disk key (via generate_and_write) MUST NOT corrupt the prior
    // backup — restoring from the blob still yields kp1.
    let dir = tempdir().unwrap();
    let path = dir.path().join("identity.key");

    let kp1 = Keypair::generate_and_write(&path).unwrap();
    let backup = kp1.export_encrypted("very-long-passphrase").unwrap();
    let pk1 = kp1.public_key();

    // Rotate after backup — the prior key bytes are gone from disk.
    let _kp2 = Keypair::generate_and_write(&path).unwrap();

    // The backup blob still imports to the original keypair regardless of disk.
    let restored = Keypair::import_encrypted(&backup, "very-long-passphrase").unwrap();
    assert_eq!(restored.public_key(), pk1);
}

#[test]
fn tofu_records_first_seen_then_known_match_across_restart() {
    // Per 2026-04-26 spec §2: the prior `tofu_flags_impersonation_across_restart`
    // test exercised the removed `DisplayNameMismatch` variant. The persistence
    // contract is still meaningful — a recorded pubkey survives reload — so
    // we keep the across-restart shape, just without the impersonation alarm.
    let dir = tempdir().unwrap();
    let tofu_path = dir.path().join("tofu.json");

    let kp_a = Keypair::generate();

    {
        let mut r = TofuRegistry::load(&tofu_path).unwrap();
        assert_eq!(
            r.check_or_record(kp_a.public_key(), Some("lx92"), 1)
                .unwrap(),
            TofuResult::FirstSeen
        );
    }

    // New process: same pubkey is now known.
    {
        let mut r = TofuRegistry::load(&tofu_path).unwrap();
        assert_eq!(
            r.check_or_record(kp_a.public_key(), Some("lx92"), 2)
                .unwrap(),
            TofuResult::KnownMatch
        );
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

// -------- Retro D3 / D9 / D-004-A public API coverage --------

use identity::{verify_jws, IdentityError};
use jsonwebtoken::{Algorithm, Header};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct BundleSigClaims {
    h: String, // hex of canonical hash
    iat: u64,
}

#[test]
fn sign_jws_roundtrip_through_public_api() {
    let kp = Keypair::generate();
    let claims = BundleSigClaims {
        h: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        iat: 1_700_000_000,
    };
    let jws = kp
        .sign_jws(&claims, &Header::new(Algorithm::EdDSA))
        .expect("sign_jws");
    let decoded = verify_jws::<BundleSigClaims>(&jws, &kp.public_key()).expect("verify_jws");
    assert_eq!(decoded.claims, claims);
    assert_eq!(decoded.header.alg, Algorithm::EdDSA);
}

#[test]
fn sign_jws_round_trip_survives_identity_key_reload() {
    // The key written to disk must produce verifiable JWS after reload.
    let dir = tempdir().unwrap();
    let path = dir.path().join("identity.key");
    let original = Keypair::load_or_create(&path).unwrap();
    let pk = original.public_key();

    let claims = BundleSigClaims {
        h: "dead".into(),
        iat: 1,
    };
    let jws = original
        .sign_jws(&claims, &Header::new(Algorithm::EdDSA))
        .unwrap();
    drop(original);

    let reloaded = Keypair::load_or_create(&path).unwrap();
    assert_eq!(reloaded.public_key(), pk);

    // Produce another JWS with the reloaded key, verify both under the same pubkey.
    let jws2 = reloaded
        .sign_jws(&claims, &Header::new(Algorithm::EdDSA))
        .unwrap();
    verify_jws::<BundleSigClaims>(&jws, &pk).expect("original jws still verifies");
    verify_jws::<BundleSigClaims>(&jws2, &pk).expect("reloaded-key jws verifies");
}

#[test]
fn sign_request_verifies_with_ed25519_dalek_verifier() {
    // Demonstrates that sign_request bytes are verifiable by an external
    // ed25519 verifier — the same shape a Cloudflare Worker would use.
    use ed25519_dalek::{Signature, Verifier, VerifyingKey};

    let kp = Keypair::generate();
    let canonical = br#"{"path":"/v1/upload","ts":1700000000}"#;
    let sig_bytes = kp.sign_request(canonical);

    let vk = VerifyingKey::from_bytes(&kp.public_key().0).unwrap();
    let sig = Signature::from_bytes(&sig_bytes);
    vk.verify(canonical, &sig).expect("signature verifies");
}

#[test]
fn verify_jws_rejects_pubkey_mismatch() {
    let signer = Keypair::generate();
    let other = Keypair::generate();
    let claims = BundleSigClaims {
        h: "x".into(),
        iat: 0,
    };
    let jws = signer
        .sign_jws(&claims, &Header::new(Algorithm::EdDSA))
        .unwrap();
    let err = verify_jws::<BundleSigClaims>(&jws, &other.public_key()).unwrap_err();
    assert!(matches!(err, IdentityError::Jws(_)));
}

#[test]
fn bundle_error_from_impl_public_surface() {
    // The From<BundleError> impl is part of the public API; exercise it via
    // the ? operator shape callers will use.
    fn wrap() -> Result<(), IdentityError> {
        Err(bundle::BundleError::Unsafe {
            kind: bundle::UnsafeKind::Path,
            detail: "../x".into(),
        })?;
        Ok(())
    }
    let err = wrap().unwrap_err();
    match err {
        IdentityError::Bundle(bundle::BundleError::Unsafe {
            kind: bundle::UnsafeKind::Path,
            detail,
        }) => {
            assert_eq!(detail, "../x");
        }
        other => panic!("expected Bundle(Unsafe {{ Path }}), got {other}"),
    }
}

#[test]
fn error_variants_display_stably() {
    // Callers (host UI, Worker responses) rely on Display output. Pin the shape.
    assert_eq!(IdentityError::Jws("x".into()).to_string(), "jws: x");
    assert_eq!(
        IdentityError::MissingSignature.to_string(),
        "missing signature"
    );
    let be = bundle::BundleError::Unsafe {
        kind: bundle::UnsafeKind::TooManyEntries,
        detail: "64".into(),
    };
    let ie: IdentityError = be.into();
    assert!(ie.to_string().starts_with("bundle:"));
}
