mod fixtures;

use fixtures::sample_bundle;
use omni_bundle::{pack, BundleLimits};
use std::io::Read;

#[test]
fn packed_manifest_matches_json_schema() {
    // Load the authoritative schema. Path is relative to the crate's Cargo.toml.
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/contracts/bundle-manifest.schema.json");
    let schema_text = std::fs::read_to_string(&schema_path)
        .expect("schema file present");
    let schema_json: serde_json::Value =
        serde_json::from_str(&schema_text).expect("schema is valid JSON");

    let validator = jsonschema::validator_for(&schema_json).expect("schema compiles");

    // Pack a realistic bundle and extract manifest.json from the zip.
    let (manifest, files) = sample_bundle();
    let bytes = pack(&manifest, &files, &BundleLimits::DEFAULT).expect("pack");

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip parse");
    let mut mf = archive.by_name("manifest.json").expect("manifest present");
    let mut buf = String::new();
    mf.read_to_string(&mut buf).expect("utf-8");
    drop(mf);

    let manifest_json: serde_json::Value =
        serde_json::from_str(&buf).expect("manifest is valid JSON");

    let errors: Vec<_> = validator.iter_errors(&manifest_json).collect();
    assert!(
        errors.is_empty(),
        "packed manifest.json violates schema: {:?}",
        errors.iter().map(|e| format!("{} at {}", e, e.instance_path)).collect::<Vec<_>>()
    );
}

#[test]
fn packed_manifest_with_resource_kinds_matches_schema() {
    use std::collections::BTreeMap;
    use omni_bundle::{BundleLimits, ResourceKind};

    let (mut manifest, files) = fixtures::sample_bundle();
    let mut kinds = BTreeMap::new();
    kinds.insert(
        "theme".into(),
        ResourceKind {
            dir: "themes".into(),
            extensions: vec![".css".into()],
            max_size_bytes: 131_072,
        },
    );
    kinds.insert(
        "sound".into(),
        ResourceKind {
            dir: "sounds".into(),
            extensions: vec![".ogg".into(), ".wav".into()],
            max_size_bytes: 524_288,
        },
    );
    manifest.resource_kinds = Some(kinds);

    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/contracts/bundle-manifest.schema.json");
    let schema_text = std::fs::read_to_string(&schema_path).expect("schema file present");
    let schema_json: serde_json::Value =
        serde_json::from_str(&schema_text).expect("schema is valid JSON");
    let validator = jsonschema::validator_for(&schema_json).expect("schema compiles");

    let bytes = omni_bundle::pack(&manifest, &files, &BundleLimits::DEFAULT).expect("pack");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip parse");
    let mut mf = archive.by_name("manifest.json").expect("manifest present");
    let mut buf = String::new();
    mf.read_to_string(&mut buf).expect("utf-8");
    drop(mf);

    let manifest_json: serde_json::Value = serde_json::from_str(&buf).expect("valid JSON");
    let errors: Vec<_> = validator.iter_errors(&manifest_json).collect();
    assert!(
        errors.is_empty(),
        "manifest with resource_kinds violates schema: {:?}",
        errors
            .iter()
            .map(|e| format!("{e} at {}", e.instance_path))
            .collect::<Vec<_>>()
    );
}
