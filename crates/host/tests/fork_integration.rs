//! Integration tests for `fork_to_local` crossing `workspace::fork` +
//! `workspace::atomic_dir` with a real on-disk layout.

use std::path::Path;

use omni_host::workspace::fork::{
    fork_to_local, ForkOrigin, ForkRequest, InstalledBundleLookup, InstalledBundleView,
};

struct OneBundle(InstalledBundleView);
impl InstalledBundleLookup for OneBundle {
    fn lookup(&self, slug: &str) -> Option<InstalledBundleView> {
        if slug == "demo" { Some(self.0.clone()) } else { None }
    }
}

fn install_fixture(root: &Path) -> InstalledBundleView {
    let dir = root.join("bundles/demo");
    std::fs::create_dir_all(dir.join("images")).unwrap();
    std::fs::write(dir.join("overlay.omni"),
        b"<html><body><script>window.__omni_sentinel=1;</script></body></html>").unwrap();
    std::fs::write(dir.join("manifest.json"), br#"{"schema_version":1}"#).unwrap();
    std::fs::write(dir.join("images/logo.png"), b"\x89PNG fake").unwrap();
    InstalledBundleView {
        path: dir,
        artifact_id: "author/demo".into(),
        content_hash: "f".repeat(64),
        bundle_name: "demo".into(),
        author_pubkey: "a".repeat(64),
        author_display_name: Some("Author".into()),
        author_fingerprint: "abcd1234".into(),
    }
}

#[test]
fn full_fork_preserves_source_and_writes_origin() {
    let root_dir = tempfile::TempDir::new().unwrap();
    let root = root_dir.path();
    let bundle = install_fixture(&root);
    let overlays = root.join("overlays");
    std::fs::create_dir_all(&overlays).unwrap();

    let before_src = snapshot(&bundle.path);
    let reg = OneBundle(bundle.clone());

    let res = fork_to_local(
        ForkRequest {
            bundle_slug: "demo".into(),
            new_overlay_name: "mine".into(),
        },
        &overlays,
        &reg,
    ).expect("fork succeeds");

    assert_eq!(res.name, "mine");
    assert!(res.path.join("overlay.omni").exists());
    assert!(res.path.join("images/logo.png").exists());
    assert!(res.path.join("manifest.json").exists(),
        "manifest.json copied verbatim for provenance");

    let origin_bytes = std::fs::read(res.path.join(".omni-origin.json")).unwrap();
    let origin: ForkOrigin = serde_json::from_slice(&origin_bytes).unwrap();
    assert_eq!(origin.version, 1);
    assert_eq!(origin.forked_from.artifact_id, "author/demo");

    let after_src = snapshot(&bundle.path);
    assert_eq!(before_src, after_src, "source bundle was mutated by fork");
}

#[test]
fn two_forks_from_same_source_are_independent() {
    let root_dir = tempfile::TempDir::new().unwrap();
    let root = root_dir.path();
    let bundle = install_fixture(&root);
    let overlays = root.join("overlays");
    std::fs::create_dir_all(&overlays).unwrap();
    let reg = OneBundle(bundle);

    let a = fork_to_local(
        ForkRequest { bundle_slug: "demo".into(), new_overlay_name: "a".into() },
        &overlays, &reg,
    ).unwrap();
    let b = fork_to_local(
        ForkRequest { bundle_slug: "demo".into(), new_overlay_name: "b".into() },
        &overlays, &reg,
    ).unwrap();

    std::fs::write(a.path.join("overlay.omni"), b"mutated").unwrap();
    let b_content = std::fs::read(b.path.join("overlay.omni")).unwrap();
    assert!(b_content.starts_with(b"<html>"),
        "fork B must be independent of fork A's edits");
}

#[test]
fn orphan_staging_dirs_are_swept() {
    let root_dir = tempfile::TempDir::new().unwrap();
    let root = root_dir.path();
    let overlays = root.join("overlays");
    std::fs::create_dir_all(&overlays).unwrap();
    let orphan = overlays.join(".omni-staging-deadbeef-cafe-1234");
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::write(orphan.join("stray.txt"), b"leftover").unwrap();

    let removed = omni_host::workspace::atomic_dir::sweep_orphans(&overlays)
        .expect("sweep ok");
    assert!(removed >= 1, "should report at least one orphan removed");
    assert!(!orphan.exists(), "orphan staging dir should be gone");
}

fn snapshot(p: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v = Vec::new();
    snap(p, p, &mut v);
    v.sort();
    v
}
fn snap(root: &Path, p: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let path = e.path();
            if let Ok(md) = e.metadata() {
                if md.is_file() {
                    let rel = path.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                    let bytes = std::fs::read(&path).unwrap_or_default();
                    out.push((rel, bytes));
                } else if md.is_dir() {
                    snap(root, &path, out);
                }
            }
        }
    }
}
