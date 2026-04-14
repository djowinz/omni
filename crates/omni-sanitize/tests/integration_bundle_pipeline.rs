//! Integration tests A–G (spec §6.2) crossing omni-bundle ↔ omni-identity
//! ↔ omni-sanitize.

use std::collections::BTreeMap;

use omni_bundle::{BundleLimits, FileEntry, ResourceKind};
use omni_identity::{pack_signed_bundle, unpack_signed_bundle, IdentityError};
use omni_sanitize::{sanitize_bundle, SanitizeError};

mod common;
use common::sha256;

/// Case A: omni_bundle::pack → unpack → sanitize_bundle — verify
/// SanitizeReport.sanitized_sha256 matches each sanitized file's content.
#[test]
fn case_a_pack_sanitize_roundtrip() {
    let (manifest, files) = common::clean_bundle();
    let limits = BundleLimits::DEFAULT;

    let bytes = omni_bundle::pack(&manifest, &files, &limits).unwrap();
    let unpack = omni_bundle::unpack(&bytes, &limits).unwrap();
    let (m2, f2) = unpack.into_map().unwrap();

    let (sanitized, report) = sanitize_bundle(&m2, f2).unwrap();
    for fr in &report.files {
        let actual = sha256(&sanitized[&fr.path]);
        assert_eq!(actual, fr.sanitized_sha256, "hash mismatch for {}", fr.path);
    }
}

/// Case B: full signed pipeline roundtrip.
#[test]
fn case_b_signed_pipeline_roundtrip() {
    let (m, f) = common::clean_bundle();
    let kp = omni_identity::Keypair::generate();
    let limits = BundleLimits::DEFAULT;

    let bytes1 = pack_signed_bundle(&m, &f, &kp, &limits).unwrap();
    let signed1 = unpack_signed_bundle(&bytes1, Some(&kp.public_key()), &limits).unwrap();
    let (m1, files1) = signed1.into_files_map();

    let (sanitized, _report) = sanitize_bundle(&m1, files1).unwrap();

    // Re-build manifest to match sanitized file content hashes.
    let mut m2 = m1.clone();
    m2.files = sanitized
        .iter()
        .map(|(p, b)| FileEntry { path: p.clone(), sha256: sha256(b) })
        .collect();
    m2.files.sort_by(|a, b| a.path.cmp(&b.path));

    let bytes2 = pack_signed_bundle(&m2, &sanitized, &kp, &limits).unwrap();
    let signed2 = unpack_signed_bundle(&bytes2, Some(&kp.public_key()), &limits).unwrap();
    assert_eq!(signed2.manifest().name, m.name);
}

/// Case C: post-sign tamper — flip a byte, unpack rejects.
#[test]
fn case_c_post_sign_tamper() {
    let (m, f) = common::clean_bundle();
    let kp = omni_identity::Keypair::generate();
    let limits = BundleLimits::DEFAULT;

    let mut bytes = pack_signed_bundle(&m, &f, &kp, &limits).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xFF;
    match unpack_signed_bundle(&bytes, None, &limits) {
        Ok(_) => panic!("tamper must abort"),
        Err(IdentityError::Bundle(_)) | Err(IdentityError::Jws(_)) => {}
        Err(other) => panic!("unexpected error variant: {other:?}"),
    }
}

/// Case D: wrong expected pubkey.
#[test]
fn case_d_wrong_key() {
    let (m, f) = common::clean_bundle();
    let (kp_a, kp_b) = common::two_keypairs();
    let limits = BundleLimits::DEFAULT;

    let bytes = pack_signed_bundle(&m, &f, &kp_a, &limits).unwrap();
    match unpack_signed_bundle(&bytes, Some(&kp_b.public_key()), &limits) {
        Ok(_) => panic!("wrong-key must abort"),
        Err(IdentityError::Jws(_)) => {}
        Err(other) => panic!("expected Jws variant, got {other:?}"),
    }
}

/// Case E: unknown resource kind declared in manifest.
#[test]
fn case_e_unknown_resource_kind() {
    let (mut m, mut f) = common::clean_bundle();
    f.insert("sounds/x.wav".to_string(), b"RIFF____WAVE".to_vec());
    m.files.push(FileEntry {
        path: "sounds/x.wav".into(),
        sha256: sha256(&f["sounds/x.wav"]),
    });
    m.files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut rk = BTreeMap::new();
    rk.insert("sounds".to_string(), ResourceKind {
        dir: "sounds".into(),
        extensions: vec!["wav".into()],
        max_size_bytes: 1024,
    });
    m.resource_kinds = Some(rk);
    let err = sanitize_bundle(&m, f).unwrap_err();
    match err {
        SanitizeError::UnknownResourceKind { kind, supported } => {
            assert_eq!(kind, "sounds");
            assert!(supported.contains(&"theme"));
        }
        other => panic!("expected UnknownResourceKind, got {other:?}"),
    }
}

/// Case F: sanitize idempotence on file-contents map.
#[test]
fn case_f_sanitize_idempotence() {
    let (m, f) = common::clean_bundle();
    let (out1, _) = sanitize_bundle(&m, f).unwrap();
    let (out2, _) = sanitize_bundle(&m, out1.clone()).unwrap();
    assert_eq!(out1, out2, "sanitize idempotent on file-contents map");
}

/// Case G: schema-version rejection wiring.
#[test]
fn case_g_schema_version_rejection() {
    let (mut m, f) = common::clean_bundle();
    m.schema_version = 2;
    let err = sanitize_bundle(&m, f).unwrap_err();
    match err {
        SanitizeError::Malformed { source, .. } => {
            let s = source.expect("source chain");
            let msg = s.to_string();
            assert!(msg.contains("SchemaVersionUnsupported"), "source message: {msg}");
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
}
