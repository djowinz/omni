mod fixtures;

use std::collections::BTreeMap;

use fixtures::sha256;
use omni_bundle::{pack, unpack, BundleLimits, FileEntry, Manifest, Tag};
use proptest::prelude::*;

fn arb_css_bytes() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 1..2048)
}

fn arb_theme_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,15}".prop_map(|s| format!("themes/{s}.css"))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn pack_unpack_roundtrips(
        themes in prop::collection::vec((arb_theme_name(), arb_css_bytes()), 0..6)
    ) {
        let overlay = b"<overlay/>".to_vec();

        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        files.insert("overlay.omni".into(), overlay.clone());
        for (name, content) in &themes {
            files.insert(name.clone(), content.clone());
        }

        let mut entries: Vec<FileEntry> = files
            .iter()
            .map(|(p, b)| FileEntry { path: p.clone(), sha256: sha256(b) })
            .collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));

        let manifest = Manifest {
            schema_version: 1,
            name: "fuzz".into(),
            version: "1.0.0".parse().unwrap(),
            omni_min_version: "0.1.0".parse().unwrap(),
            description: "".into(),
            tags: vec![Tag::new("dark").unwrap()],
            license: "MIT".into(),
            entry_overlay: "overlay.omni".into(),
            default_theme: None,
            sensor_requirements: vec![],
            files: entries,
            resource_kinds: None,
        };

        let bytes = pack(&manifest, &files, &BundleLimits::DEFAULT).expect("pack");
        let (m2, f2) = unpack(&bytes, &BundleLimits::DEFAULT)
            .expect("unpack")
            .into_map()
            .expect("collect");
        prop_assert_eq!(m2, manifest);
        prop_assert_eq!(f2, files);
    }
}
