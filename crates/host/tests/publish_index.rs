//! Integration coverage for `share::publish_index` (upload-flow redesign §8.2).

use omni_host::share::publish_index::{read, write, PublishIndex, PublishIndexEntry};
use tempfile::tempdir;

fn entry(name: &str, version: &str) -> PublishIndexEntry {
    PublishIndexEntry {
        pubkey_hex: "ab".into(),
        kind: "overlay".into(),
        name: name.into(),
        artifact_id: format!("ov_{name}"),
        last_version: version.into(),
        last_published_at: "2026-04-18T00:00:00Z".into(),
    }
}

#[test]
fn upsert_then_lookup() {
    let mut idx = PublishIndex::default();
    let e = entry("marathon", "1.0.0");
    idx.upsert(e.clone());
    assert_eq!(idx.lookup("ab", "overlay", "marathon"), Some(&e));
    assert_eq!(idx.lookup("ab", "theme", "marathon"), None);
    assert_eq!(idx.lookup("zz", "overlay", "marathon"), None);
}

#[test]
fn upsert_replaces_existing_entry() {
    let mut idx = PublishIndex::default();
    let e1 = entry("marathon", "1.0.0");
    let e2 = entry("marathon", "1.1.0");
    idx.upsert(e1);
    idx.upsert(e2.clone());
    assert_eq!(idx.entries.len(), 1, "upsert must not duplicate");
    assert_eq!(idx.lookup("ab", "overlay", "marathon"), Some(&e2));
}

#[test]
fn upsert_appends_distinct_keys() {
    let mut idx = PublishIndex::default();
    idx.upsert(entry("marathon", "1.0.0"));
    idx.upsert(entry("sprint", "1.0.0"));
    assert_eq!(idx.entries.len(), 2);
    assert!(idx.lookup("ab", "overlay", "marathon").is_some());
    assert!(idx.lookup("ab", "overlay", "sprint").is_some());
}

#[test]
fn read_missing_file_returns_empty_index() {
    let dir = tempdir().expect("tempdir");
    let p = dir.path().join("does-not-exist.json");
    let idx = read(&p).expect("missing file is not an error");
    assert!(idx.entries.is_empty());
}

#[test]
fn read_write_roundtrip() {
    let dir = tempdir().expect("tempdir");
    let p = dir.path().join("publish-index.json");
    let mut idx = PublishIndex::default();
    idx.upsert(entry("marathon", "1.0.0"));
    idx.upsert(entry("sprint", "0.4.2"));
    write(&p, &idx).expect("write");
    let back = read(&p).expect("read");
    assert_eq!(back, idx);
}

#[test]
fn write_creates_parent_dir() {
    // Defensive: index_path() points at %APPDATA%/Omni/, which may not exist
    // on a fresh-install host. write() must mkdir its parent.
    let dir = tempdir().expect("tempdir");
    let p = dir.path().join("nested").join("dir").join("publish-index.json");
    let idx = PublishIndex::default();
    write(&p, &idx).expect("write must mkdir parent");
    assert!(p.exists());
}

#[test]
fn malformed_json_treated_as_empty_index() {
    // Per the module's design note: the index is a derived cache and must
    // never block dialog open. Garbage on disk degrades to an empty index.
    let dir = tempdir().expect("tempdir");
    let p = dir.path().join("publish-index.json");
    std::fs::write(&p, b"{not valid json").expect("write garbage");
    let idx = read(&p).expect("garbage tolerated");
    assert!(idx.entries.is_empty());
}

#[test]
fn remove_returns_false_when_no_match() {
    let mut idx = PublishIndex::default();
    idx.upsert(entry("marathon", "1.0.0"));
    assert!(!idx.remove("zz", "overlay", "marathon"));
    assert_eq!(idx.entries.len(), 1);
}

#[test]
fn remove_returns_true_and_drops_entry() {
    let mut idx = PublishIndex::default();
    idx.upsert(entry("marathon", "1.0.0"));
    assert!(idx.remove("ab", "overlay", "marathon"));
    assert!(idx.entries.is_empty());
}
