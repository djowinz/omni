//! Integration tests for `fork_to_local` crossing `workspace::fork` +
//! `workspace::atomic_dir` with a real on-disk layout.

use std::collections::HashMap;
use std::path::Path;

use omni_host::workspace::fork::{
    fork_to_local, ForkOrigin, ForkRequest, InstalledBundleLookup, InstalledBundleView,
};

struct OneBundle(InstalledBundleView);
impl InstalledBundleLookup for OneBundle {
    fn lookup(&self, slug: &str) -> Option<InstalledBundleView> {
        if slug == "demo" {
            Some(self.0.clone())
        } else {
            None
        }
    }
}

struct StubLookup(HashMap<String, InstalledBundleView>);
impl InstalledBundleLookup for StubLookup {
    fn lookup(&self, slug: &str) -> Option<InstalledBundleView> {
        self.0.get(slug).cloned()
    }
}

fn install_fixture(root: &Path) -> InstalledBundleView {
    let dir = root.join("bundles/demo");
    std::fs::create_dir_all(dir.join("images")).unwrap();
    std::fs::write(
        dir.join("overlay.omni"),
        b"<html><body><script>window.__omni_sentinel=1;</script></body></html>",
    )
    .unwrap();
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
    let bundle = install_fixture(root);
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
    )
    .expect("fork succeeds");

    assert_eq!(res.name, "mine");
    assert!(res.path.join("overlay.omni").exists());
    assert!(res.path.join("images/logo.png").exists());
    assert!(
        res.path.join("manifest.json").exists(),
        "manifest.json copied verbatim for provenance"
    );

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
    let bundle = install_fixture(root);
    let overlays = root.join("overlays");
    std::fs::create_dir_all(&overlays).unwrap();
    let reg = OneBundle(bundle);

    let a = fork_to_local(
        ForkRequest {
            bundle_slug: "demo".into(),
            new_overlay_name: "a".into(),
        },
        &overlays,
        &reg,
    )
    .unwrap();
    let b = fork_to_local(
        ForkRequest {
            bundle_slug: "demo".into(),
            new_overlay_name: "b".into(),
        },
        &overlays,
        &reg,
    )
    .unwrap();

    std::fs::write(a.path.join("overlay.omni"), b"mutated").unwrap();
    let b_content = std::fs::read(b.path.join("overlay.omni")).unwrap();
    assert!(
        b_content.starts_with(b"<html>"),
        "fork B must be independent of fork A's edits"
    );
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

    let removed = omni_host::workspace::atomic_dir::sweep_orphans(&overlays).expect("sweep ok");
    assert!(removed >= 1, "should report at least one orphan removed");
    assert!(!orphan.exists(), "orphan staging dir should be gone");
}

#[test]
fn fork_of_installed_bundle_writes_beautified_files() {
    // Fixture: an installed bundle directory containing a minified .css
    // and a minified .omni. We construct it on disk, then fork it, then
    // assert the forked files are pretty-printed.
    use std::fs;
    use tempfile::TempDir;

    let root = TempDir::new().unwrap();
    let bundle_dir = root.path().join("bundles/author/minified");
    fs::create_dir_all(bundle_dir.join("themes")).unwrap();

    // Minified bodies — what install would have written.
    fs::write(bundle_dir.join("themes/dark.css"), b"body{color:red;margin:0}").unwrap();
    fs::write(
        bundle_dir.join("overlay.omni"),
        b"<widget><template><div><span>a</span></div></template><style>body{color:red;margin:0}</style></widget>",
    )
    .unwrap();
    fs::write(bundle_dir.join("manifest.json"), b"{\"name\":\"x\"}").unwrap();

    let view = InstalledBundleView {
        path: bundle_dir.clone(),
        artifact_id: "author/minified".into(),
        content_hash: "a".repeat(64),
        bundle_name: "minified".into(),
        author_pubkey: "b".repeat(64),
        author_display_name: Some("Author".into()),
        author_fingerprint: "c".repeat(8),
    };
    let mut map = HashMap::new();
    map.insert("author/minified".into(), view);
    let lookup = StubLookup(map);

    let overlays_root = root.path().join("overlays");
    fs::create_dir_all(&overlays_root).unwrap();

    let req = ForkRequest {
        bundle_slug: "author/minified".into(),
        new_overlay_name: "my-fork".into(),
    };
    let result = fork_to_local(req, &overlays_root, &lookup).expect("fork must succeed");

    // Assertions on the resulting fork.
    let css = fs::read_to_string(result.path.join("themes/dark.css"))
        .expect("forked css must exist");
    assert!(css.contains('\n'), "css must be pretty-printed: {css:?}");

    let omni = fs::read_to_string(result.path.join("overlay.omni"))
        .expect("forked omni must exist");
    let style_open = omni.find("<style>").unwrap();
    let style_close = omni.find("</style>").unwrap();
    let style_body = &omni[style_open + "<style>".len()..style_close];
    assert!(
        style_body.contains('\n'),
        "<style> body must be multi-line in fork: {style_body:?}"
    );

    let tpl_open = omni.find("<template>").unwrap();
    let tpl_close = omni.find("</template>").unwrap();
    let tpl_body = &omni[tpl_open + "<template>".len()..tpl_close];
    assert!(
        tpl_body.contains('\n'),
        "<template> body must be multi-line in fork: {tpl_body:?}"
    );

    // manifest.json: pass-through, byte-equal.
    let manifest = fs::read(result.path.join("manifest.json")).unwrap();
    assert_eq!(manifest, b"{\"name\":\"x\"}", "manifest must pass through unchanged");

    // .omni-origin.json: written by fork itself, must be valid JSON with origin.
    let origin_bytes = fs::read(result.path.join(".omni-origin.json")).unwrap();
    let origin: serde_json::Value = serde_json::from_slice(&origin_bytes).expect("origin parses");
    assert_eq!(origin["forked_from"]["artifact_id"], "author/minified");
    assert_eq!(origin["forked_from"]["content_hash"], "a".repeat(64));
}

#[test]
#[tracing_test::traced_test]
fn fork_with_invalid_css_falls_back_to_raw_bytes() {
    use std::fs;
    use tempfile::TempDir;

    let root = TempDir::new().unwrap();
    let bundle_dir = root.path().join("bundles/author/broken");
    fs::create_dir_all(bundle_dir.join("themes")).unwrap();

    // Garbage that fails lightningcss::StyleSheet::parse.
    let broken = b"body{:::not valid css";
    fs::write(bundle_dir.join("themes/dark.css"), broken).unwrap();

    let view = InstalledBundleView {
        path: bundle_dir.clone(),
        artifact_id: "author/broken".into(),
        content_hash: "a".repeat(64),
        bundle_name: "broken".into(),
        author_pubkey: "b".repeat(64),
        author_display_name: None,
        author_fingerprint: "c".repeat(8),
    };
    let mut map = HashMap::new();
    map.insert("author/broken".into(), view);
    let lookup = StubLookup(map);

    let overlays_root = root.path().join("overlays");
    fs::create_dir_all(&overlays_root).unwrap();

    let req = ForkRequest {
        bundle_slug: "author/broken".into(),
        new_overlay_name: "my-broken-fork".into(),
    };
    let result = fork_to_local(req, &overlays_root, &lookup).expect("fork must succeed despite bad css");

    let on_disk = fs::read(result.path.join("themes/dark.css")).unwrap();
    assert_eq!(on_disk, broken, "fork must fall back to raw bytes when beautify fails");
    assert!(
        logs_contain("fork beautify failed"),
        "expected fall-back warning to be logged"
    );
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
                    let rel = path
                        .strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    let bytes = std::fs::read(&path).unwrap_or_default();
                    out.push((rel, bytes));
                } else if md.is_dir() {
                    snap(root, &path, out);
                }
            }
        }
    }
}
