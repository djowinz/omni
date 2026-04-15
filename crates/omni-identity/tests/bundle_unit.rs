//! Contract-level tests for omni-identity's bundle-signing surface
//! (retro-005 D4 / writing-lessons §9).

use std::collections::BTreeMap;

use omni_bundle::{BundleLimits, FileEntry, Manifest};
use omni_identity::{pack_signed_bundle, unpack_signed_bundle, IdentityError, Keypair};
use sha2::{Digest, Sha256};

fn sha256(b: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(b);
    h.finalize().into()
}

/// Extract the Err value from a Result whose Ok type does not implement Debug.
fn unwrap_err_no_debug<T, E>(r: Result<T, E>, msg: &str) -> E {
    match r {
        Ok(_) => panic!("{msg}"),
        Err(e) => e,
    }
}

fn fixture() -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let overlay = br#"<overlay><template><div/></template></overlay>"#.to_vec();
    let css = b"body{color:red}".to_vec();
    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), overlay.clone());
    files.insert("themes/default.css".to_string(), css.clone());
    let manifest = Manifest {
        schema_version: 1,
        name: "t".into(),
        version: semver::Version::new(0, 1, 0),
        omni_min_version: semver::Version::new(0, 1, 0),
        description: String::new(),
        tags: vec![],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/default.css".into()),
        sensor_requirements: vec![],
        files: vec![
            FileEntry {
                path: "overlay.omni".into(),
                sha256: sha256(&overlay),
            },
            FileEntry {
                path: "themes/default.css".into(),
                sha256: sha256(&css),
            },
        ],
        resource_kinds: None,
    };
    (manifest, files)
}

#[test]
fn pack_unpack_roundtrip() {
    let (m, f) = fixture();
    let kp = Keypair::generate();
    let limits = BundleLimits::DEFAULT;

    let bytes = pack_signed_bundle(&m, &f, &kp, &limits).unwrap();
    let signed = unpack_signed_bundle(&bytes, Some(&kp.public_key()), &limits).unwrap();

    assert_eq!(signed.manifest().name, "t");
    // author_pubkey() returns &PublicKey; .0 is pub [u8; 32]
    assert_eq!(signed.author_pubkey().0, kp.public_key().0);
    assert_eq!(signed.fingerprint(), kp.public_key().fingerprint());

    let (_m2, files_map) = signed.into_files_map();
    assert!(files_map.contains_key("overlay.omni"));
    assert!(files_map.contains_key("themes/default.css"));
    assert!(
        !files_map.contains_key("signature.jws"),
        "signature must be stripped"
    );
}

#[test]
fn unpack_rejects_tampered_zip() {
    let (m, f) = fixture();
    let kp = Keypair::generate();
    let limits = BundleLimits::DEFAULT;

    let mut bytes = pack_signed_bundle(&m, &f, &kp, &limits).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;

    let err: IdentityError = unwrap_err_no_debug(
        unpack_signed_bundle(&bytes, None, &limits),
        "tampered zip must be rejected",
    );
    assert!(
        matches!(err, IdentityError::Bundle(_) | IdentityError::Jws(_)),
        "tamper must abort; got {err:?}"
    );
}

#[test]
fn unpack_rejects_wrong_expected_pubkey() {
    let (m, f) = fixture();
    let kp_a = Keypair::generate();
    let kp_b = Keypair::generate();
    let limits = BundleLimits::DEFAULT;

    let bytes = pack_signed_bundle(&m, &f, &kp_a, &limits).unwrap();
    let err: IdentityError = unwrap_err_no_debug(
        unpack_signed_bundle(&bytes, Some(&kp_b.public_key()), &limits),
        "wrong expected pubkey must be rejected",
    );
    assert!(matches!(err, IdentityError::Jws(_)));
}

#[test]
fn unpack_rejects_missing_signature() {
    let (m, f) = fixture();
    let limits = BundleLimits::DEFAULT;

    // Pack without signature — use omni_bundle::pack directly (no JWS file).
    let bytes = omni_bundle::pack(&m, &f, &limits).unwrap();

    let err: IdentityError = unwrap_err_no_debug(
        unpack_signed_bundle(&bytes, None, &limits),
        "bundle without signature must be rejected",
    );
    assert!(matches!(
        err,
        IdentityError::MissingSignature | IdentityError::Bundle(_)
    ));
}

#[test]
fn fingerprint_matches_pubkey_derivation() {
    let kp = Keypair::generate();
    let (m, f) = fixture();
    let bytes = pack_signed_bundle(&m, &f, &kp, &BundleLimits::DEFAULT).unwrap();
    let signed = unpack_signed_bundle(&bytes, None, &BundleLimits::DEFAULT).unwrap();
    assert_eq!(signed.fingerprint(), kp.public_key().fingerprint());
    assert_eq!(signed.author_pubkey().0, kp.public_key().0);
}
