mod fixtures;

use fixtures::sample_bundle;
use omni_bundle::{canonical_hash, pack, unpack, BundleLimits};

#[test]
fn pack_then_unpack_yields_same_manifest_and_files() {
    let (manifest, files) = sample_bundle();
    let bytes = pack(&manifest, &files, &BundleLimits::DEFAULT).expect("pack");
    let (m2, f2) = unpack(&bytes, &BundleLimits::DEFAULT)
        .expect("unpack")
        .into_map()
        .expect("collect");
    assert_eq!(m2, manifest);
    assert_eq!(f2, files);
}

#[test]
fn canonical_hash_survives_roundtrip() {
    let (manifest, files) = sample_bundle();
    let h_before = canonical_hash(&manifest, &files);
    let bytes = pack(&manifest, &files, &BundleLimits::DEFAULT).expect("pack");
    let (m2, f2) = unpack(&bytes, &BundleLimits::DEFAULT)
        .expect("unpack")
        .into_map()
        .expect("collect");
    let h_after = canonical_hash(&m2, &f2);
    assert_eq!(h_before, h_after);
}
