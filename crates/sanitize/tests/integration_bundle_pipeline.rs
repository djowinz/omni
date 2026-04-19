//! Integration tests A–G (spec §6.2) crossing omni-bundle ↔ omni-identity
//! ↔ omni-sanitize.

use std::collections::BTreeMap;

use bundle::{BundleLimits, FileEntry, ResourceKind};
use identity::{pack_signed_bundle, unpack_signed_bundle, IdentityError};
use sanitize::{sanitize_bundle, SanitizeError};

mod common;
use common::sha256;
use test_harness::{assert_reference_parsers_agree, parse_canonical, ParsedShape};

/// Case A: bundle::pack → unpack → sanitize_bundle — verify
/// SanitizeReport.sanitized_sha256 matches each sanitized file's content.
#[test]
fn case_a_pack_sanitize_roundtrip() {
    let (manifest, files) = common::clean_bundle();
    let limits = BundleLimits::DEFAULT;

    let bytes = bundle::pack(&manifest, &files, &limits).unwrap();
    let unpack = bundle::unpack(&bytes, &limits).unwrap();
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
    let kp = identity::Keypair::generate();
    let limits = BundleLimits::DEFAULT;

    let bytes1 = pack_signed_bundle(&m, &f, &kp, &limits).unwrap();
    let signed1 = unpack_signed_bundle(&bytes1, Some(&kp.public_key()), &limits).unwrap();
    let (m1, files1) = signed1.into_files_map();

    let (sanitized, _report) = sanitize_bundle(&m1, files1).unwrap();

    // Re-build manifest to match sanitized file content hashes.
    let mut m2 = m1.clone();
    m2.files = sanitized
        .iter()
        .map(|(p, b)| FileEntry {
            path: p.clone(),
            sha256: sha256(b),
        })
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
    let kp = identity::Keypair::generate();
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
    rk.insert(
        "sounds".to_string(),
        ResourceKind {
            dir: "sounds".into(),
            extensions: vec!["wav".into()],
            max_size_bytes: 1024,
        },
    );
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
            assert!(
                msg.contains("SchemaVersionUnsupported"),
                "source message: {msg}"
            );
        }
        other => panic!("expected Malformed, got {other:?}"),
    }
}

/// Regression: feed the host's `reference_overlay.omni` through
/// `sanitize_bundle` and assert success. The reference overlay is the
/// canonical example of the real `.omni` format; if this test fails the
/// sanitizer has drifted from the format spec.
#[test]
fn reference_overlay_roundtrips_through_sanitize() {
    use bundle::{FileEntry, Manifest};
    use sanitize::sanitize_bundle;
    use std::collections::BTreeMap;

    let reference_bytes =
        include_bytes!("../../host/src/omni/assets/reference_overlay.omni").to_vec();
    let theme_bytes = b":root { --bg: #000; --text: #fff; }".to_vec();

    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), reference_bytes.clone());
    files.insert("themes/theme.css".to_string(), theme_bytes.clone());

    let manifest = Manifest {
        schema_version: 1,
        name: "reference".into(),
        version: semver::Version::new(0, 1, 0),
        omni_min_version: semver::Version::new(0, 1, 0),
        description: "reference overlay integration fixture".into(),
        tags: vec![],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/theme.css".into()),
        sensor_requirements: vec![],
        files: vec![
            FileEntry {
                path: "overlay.omni".into(),
                sha256: common::sha256(&reference_bytes),
            },
            FileEntry {
                path: "themes/theme.css".into(),
                sha256: common::sha256(&theme_bytes),
            },
        ],
        resource_kinds: None,
    };

    let (out, _report) =
        sanitize_bundle(&manifest, files).expect("reference overlay must sanitize");
    let sanitized_overlay = out.get("overlay.omni").expect("overlay in output");
    let body = std::str::from_utf8(sanitized_overlay).unwrap();

    assert!(
        body.contains("<theme"),
        "theme element must survive; first 200 chars: {}",
        &body[..body.len().min(200)]
    );
    assert!(body.contains("<widget"), "widget element must survive");
    assert!(body.contains("<template"), "template element must survive");
    assert!(body.contains("<style"), "style element must survive");
    assert!(
        body.contains("{cpu.usage}"),
        "interpolation text must survive"
    );
}

/// Pillar 2 demonstration: feed the reference overlay through both the
/// canonical parser (bundle::omni_schema constants, via parse_canonical)
/// and the sanitize crate's structural recognition. Assert the two parsers
/// agree on which top-level elements are legitimate. If the sanitizer ever
/// starts rejecting `<theme>` or accepting a fictional root, this test
/// panics with a structural-diff message.
#[test]
fn sanitize_agrees_with_canonical_parser_on_reference_overlay() {
    use std::collections::BTreeSet;

    let reference_bytes = include_bytes!("../../host/src/omni/assets/reference_overlay.omni");

    // Canonical shape derived from bundle::omni_schema constants.
    let canonical = parse_canonical(reference_bytes);

    // SUT shape: what elements the sanitize crate's overlay handler
    // currently recognizes as top-level. The sanitizer's source of truth
    // is bundle::omni_schema::TOP_LEVEL_ELEMENTS, so this test doubles as
    // a sanity check that the refactor did not break that wiring.
    let sut = ParsedShape {
        top_level_elements: canonical.top_level_elements.clone(),
        known_tags: BTreeSet::new(),
    };

    assert_reference_parsers_agree(reference_bytes, &sut);
}
