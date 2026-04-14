mod fixtures;

use fixtures::{sha256, test_zip_opts};
use omni_bundle::{pack, unpack, BundleLimits, FileEntry, Manifest, Tag};
use std::collections::BTreeMap;

#[test]
fn realistic_bundle_round_trips_under_budget() {
    let _ = test_zip_opts(); // silence unused warning if fixture imports drift

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();

    // 1 overlay (~5 KB of XML)
    let overlay = make_overlay(5_000);
    files.insert("overlay.omni".into(), overlay);

    // 6 CSS themes, ~8 KB each
    for i in 0..6 {
        files.insert(
            format!("themes/theme{i}.css"),
            make_css(8_000, i as u8),
        );
    }

    // 2 fonts, ~400 KB each (well under 1.5 MB font cap).
    // Use pseudo-random bytes so compression ratio stays well under 100x.
    for i in 0..2 {
        files.insert(format!("fonts/font{i}.ttf"), pseudo_random_bytes(400_000, i as u64));
    }

    // 4 PNG images, ~200 KB each (under 1 MB image cap).
    // Use pseudo-random bytes so compression ratio stays well under 100x.
    for i in 0..4 {
        files.insert(format!("images/img{i}.png"), pseudo_random_bytes(200_000, 10 + i as u64));
    }

    let mut entries: Vec<FileEntry> = files
        .iter()
        .map(|(p, b)| FileEntry { path: p.clone(), sha256: sha256(b) })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));

    let manifest = Manifest {
        schema_version: 1,
        name: "realistic".into(),
        version: "1.0.0".parse().unwrap(),
        omni_min_version: "0.1.0".parse().unwrap(),
        description: "realistic-size bundle under budget".into(),
        tags: vec![Tag::new("dark").unwrap(), Tag::new("gaming").unwrap()],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/theme0.css".into()),
        sensor_requirements: vec!["cpu.usage".into(), "gpu.usage".into()],
        files: entries,
    };

    let bytes = pack(&manifest, &files, &BundleLimits::DEFAULT).expect("pack realistic");
    assert!(
        (bytes.len() as u64) <= BundleLimits::DEFAULT.max_bundle_compressed,
        "realistic bundle exceeded max_bundle_compressed: {} bytes > {}",
        bytes.len(),
        BundleLimits::DEFAULT.max_bundle_compressed
    );

    let (m2, f2) = unpack(&bytes, &BundleLimits::DEFAULT).expect("unpack realistic");
    assert_eq!(m2, manifest);
    assert_eq!(f2, files);
}

/// LCG pseudo-random byte generator — produces incompressible-looking data
/// without pulling in any external crate.
fn pseudo_random_bytes(size: usize, seed: u64) -> Vec<u8> {
    let mut state = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        // xorshift64
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let bytes = state.to_le_bytes();
        let take = (size - out.len()).min(8);
        out.extend_from_slice(&bytes[..take]);
    }
    out
}

fn make_overlay(size: usize) -> Vec<u8> {
    let mut s = String::from("<overlay>");
    while s.len() < size {
        s.push_str("<widget type='text' sensor='cpu.usage'/>");
    }
    s.push_str("</overlay>");
    s.into_bytes()
}

fn make_css(size: usize, seed: u8) -> Vec<u8> {
    let mut s = format!(":root {{ --accent: #{seed:02x}{seed:02x}{seed:02x}; }}\n");
    while s.len() < size {
        s.push_str(".widget { color: var(--accent); padding: 4px; }\n");
    }
    s.into_bytes()
}
