mod fixtures;

use bundle::{pack, BundleLimits};
use fixtures::sample_bundle;

#[test]
fn pack_is_byte_deterministic() {
    let (manifest, files) = sample_bundle();
    let a = pack(&manifest, &files, &BundleLimits::DEFAULT).expect("pack a");
    let b = pack(&manifest, &files, &BundleLimits::DEFAULT).expect("pack b");
    assert_eq!(a, b, "pack must be byte-deterministic");
}
