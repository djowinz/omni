mod fixtures;

use fixtures::{sample_bundle, sha256};
use omni_bundle::{pack, unpack, BundleError, BundleLimits, FileEntry, IntegrityKind, Manifest, Tag, UnsafeKind};

#[test]
fn manifest_missing_is_reported() {
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::<u8>::new()));
    let opts = fixtures::test_zip_opts();
    use std::io::Write;
    zw.start_file("overlay.omni", opts).unwrap();
    zw.write_all(b"<x/>").unwrap();
    let bytes = zw.finish().unwrap().into_inner();
    let err = unpack(&bytes, &BundleLimits::DEFAULT).unwrap_err();
    assert!(
        matches!(err, BundleError::Integrity { kind: IntegrityKind::ManifestMissing, .. }),
        "{err:?}"
    );
}

#[test]
fn file_missing_and_orphan_detected_on_pack() {
    let (mut m, mut f) = sample_bundle();
    m.files.push(FileEntry { path: "themes/ghost.css".into(), sha256: [0u8; 32] });
    let err = pack(&m, &f, &BundleLimits::DEFAULT).unwrap_err();
    assert!(
        matches!(
            err,
            BundleError::Integrity { kind: IntegrityKind::FileMissing, ref detail }
            if detail == "themes/ghost.css"
        ),
        "{err:?}"
    );

    m.files.pop();
    f.insert("themes/orphan.css".into(), b"body{}".to_vec());
    let err = pack(&m, &f, &BundleLimits::DEFAULT).unwrap_err();
    assert!(
        matches!(
            err,
            BundleError::Integrity { kind: IntegrityKind::FileOrphan, ref detail }
            if detail == "themes/orphan.css"
        ),
        "{err:?}"
    );
}

#[test]
fn hash_mismatch_rejected_on_pack() {
    let (mut m, f) = sample_bundle();
    m.files[0].sha256 = [0xff; 32];
    let err = pack(&m, &f, &BundleLimits::DEFAULT).unwrap_err();
    assert!(
        matches!(err, BundleError::Integrity { kind: IntegrityKind::HashMismatch, .. }),
        "{err:?}"
    );
}

#[test]
fn size_exceeded_rejected() {
    let (mut m, mut f) = sample_bundle();
    // Create a file that alone exceeds max_bundle_uncompressed (10 MB default).
    // Pseudo-random bytes so the zip doesn't compress and trip zip-bomb check.
    let mut big = vec![0u8; 10_485_761];
    let mut x: u64 = 0x12345678;
    for byte in big.iter_mut() {
        x = x.wrapping_mul(2862933555777941757).wrapping_add(3037000493);
        *byte = (x >> 24) as u8;
    }
    f.insert("themes/big.css".into(), big.clone());
    m.files.push(FileEntry { path: "themes/big.css".into(), sha256: sha256(&big) });
    let err = pack(&m, &f, &BundleLimits::DEFAULT).unwrap_err();
    assert!(matches!(err, BundleError::Unsafe { kind: UnsafeKind::SizeExceeded, .. }), "{err:?}");
}

#[test]
fn unsafe_path_rejected() {
    let (mut m, mut f) = sample_bundle();
    let v = b"x".to_vec();
    f.insert("../evil.css".into(), v.clone());
    m.files.push(FileEntry { path: "../evil.css".into(), sha256: sha256(&v) });
    let err = pack(&m, &f, &BundleLimits::DEFAULT).unwrap_err();
    assert!(
        matches!(err, BundleError::Unsafe { kind: UnsafeKind::Path, .. }),
        "{err:?}"
    );
}

#[test]
fn too_many_entries_rejected() {
    let (mut m, mut f) = sample_bundle();
    for i in 0..BundleLimits::DEFAULT.max_entries {
        let name = format!("themes/t{i}.css");
        let content = format!("/*{i}*/").into_bytes();
        m.files.push(FileEntry { path: name.clone(), sha256: sha256(&content) });
        f.insert(name, content);
    }
    let err = pack(&m, &f, &BundleLimits::DEFAULT).unwrap_err();
    assert!(
        matches!(err, BundleError::Unsafe { kind: UnsafeKind::TooManyEntries, .. }),
        "{err:?}"
    );
}

#[test]
fn invalid_tag_rejected_on_unpack() {
    let manifest_json = serde_json::json!({
        "schema_version": 1,
        "name": "x",
        "version": "1.0.0",
        "omni_min_version": "0.1.0",
        "description": "",
        "tags": ["NotValid Tag Contains Spaces"],
        "license": "MIT",
        "entry_overlay": "overlay.omni",
        "files": [{
            "path": "overlay.omni",
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
        }]
    });
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::<u8>::new()));
    let opts = fixtures::test_zip_opts();
    use std::io::Write;
    zw.start_file("manifest.json", opts).unwrap();
    zw.write_all(serde_json::to_vec(&manifest_json).unwrap().as_slice()).unwrap();
    zw.start_file("overlay.omni", opts).unwrap();
    zw.write_all(b"<x/>").unwrap();
    let bytes = zw.finish().unwrap().into_inner();
    let err = unpack(&bytes, &BundleLimits::DEFAULT).unwrap_err();
    assert!(matches!(err, BundleError::Malformed { .. }), "{err:?}");
}

#[test]
fn zip_bomb_or_json_rejected_on_unpack() {
    let zeros = vec![0u8; 1_000_000];
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::<u8>::new()));
    let opts = fixtures::test_zip_opts();
    use std::io::Write;
    zw.start_file("manifest.json", opts).unwrap();
    zw.write_all(b"{}").unwrap();
    zw.start_file("overlay.omni", opts).unwrap();
    zw.write_all(&zeros).unwrap();
    let bytes = zw.finish().unwrap().into_inner();
    let err = unpack(&bytes, &BundleLimits::DEFAULT).unwrap_err();
    assert!(
        matches!(
            err,
            BundleError::Unsafe { kind: UnsafeKind::ZipBomb, .. } | BundleError::Malformed { .. }
        ),
        "{err:?}"
    );
}

#[test]
fn orphan_entry_rejected_on_unpack() {
    let (manifest, files) = sample_bundle();
    let packed = pack(&manifest, &files, &BundleLimits::DEFAULT).unwrap();
    let bytes = {
        let cursor = std::io::Cursor::new(packed);
        let mut rz = zip::ZipArchive::new(cursor).unwrap();
        let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::<u8>::new()));
        let opts = fixtures::test_zip_opts();
        for i in 0..rz.len() {
            let mut f = rz.by_index(i).unwrap();
            let name = f.name().to_string();
            zw.start_file(name, opts).unwrap();
            std::io::copy(&mut f, &mut zw).unwrap();
        }
        use std::io::Write;
        zw.start_file("themes/orphan.css", opts).unwrap();
        zw.write_all(b"/* orphan */").unwrap();
        zw.finish().unwrap().into_inner()
    };
    let u = unpack(&bytes, &BundleLimits::DEFAULT).expect("initial unpack ok");
    let err = u.into_map().expect_err("expected orphan error");
    assert!(
        matches!(err, BundleError::Integrity { kind: IntegrityKind::FileOrphan, .. }),
        "{err:?}"
    );
}

#[test]
fn hash_mismatch_detected_on_unpack() {
    let overlay = b"<x/>".to_vec();
    let bad_sha = [0xbbu8; 32];
    let m = Manifest {
        schema_version: 1,
        name: "x".into(),
        version: "1.0.0".parse().unwrap(),
        omni_min_version: "0.1.0".parse().unwrap(),
        description: "".into(),
        tags: vec![Tag::new("dark").unwrap()],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: None,
        sensor_requirements: vec![],
        files: vec![FileEntry { path: "overlay.omni".into(), sha256: bad_sha }],
        resource_kinds: None,
    };
    let mut zw = zip::ZipWriter::new(std::io::Cursor::new(Vec::<u8>::new()));
    let opts = fixtures::test_zip_opts();
    use std::io::Write;
    let manifest_bytes = serde_json::to_vec(&m).unwrap();
    zw.start_file("manifest.json", opts).unwrap();
    zw.write_all(&manifest_bytes).unwrap();
    zw.start_file("overlay.omni", opts).unwrap();
    zw.write_all(&overlay).unwrap();
    let bytes = zw.finish().unwrap().into_inner();
    let u = unpack(&bytes, &BundleLimits::DEFAULT).expect("initial unpack ok");
    let err = u.into_map().expect_err("expected hash mismatch");
    assert!(
        matches!(err, BundleError::Integrity { kind: IntegrityKind::HashMismatch, .. }),
        "{err:?}"
    );
}
