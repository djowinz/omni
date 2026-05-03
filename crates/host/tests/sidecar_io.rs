//! Integration coverage for `share::sidecar` (upload-flow redesign §8.1).

use omni_host::share::sidecar::{
    read_sidecar, read_theme_sidecar, write_sidecar, write_theme_sidecar, PublishSidecar,
    SIDECAR_FILENAME, THEME_SIDECAR_SUFFIX,
};
use tempfile::tempdir;

fn sample() -> PublishSidecar {
    PublishSidecar {
        artifact_id: "ov_01J8XKZ".into(),
        author_pubkey_hex: "abcdef0123".into(),
        version: "1.3.0".into(),
        last_published_at: "2026-04-18T18:12:44Z".into(),
        description: "test desc".into(),
        tags: vec!["a".into(), "b".into()],
        license: "MIT".into(),
    }
}

#[test]
fn write_and_read_overlay_sidecar() {
    let dir = tempdir().expect("tempdir");
    let overlay_dir = dir.path().join("overlays").join("marathon-hud");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir overlay_dir");
    let s = sample();
    write_sidecar(&overlay_dir, &s).expect("write");
    let back = read_sidecar(&overlay_dir).expect("read");
    assert_eq!(back, Some(s));
    // INV-7.5.3: the expanded fields must persist intact through the
    // write→read roundtrip so the upload dialog can prefill Step 2 on update
    // mode without a worker round-trip.
    let back = read_sidecar(&overlay_dir).expect("read");
    assert_eq!(back.as_ref().unwrap().description, "test desc");
    assert_eq!(
        back.as_ref().unwrap().tags,
        vec!["a".to_string(), "b".to_string()]
    );
    assert_eq!(back.as_ref().unwrap().license, "MIT");
}

#[test]
fn read_returns_none_when_missing() {
    let dir = tempdir().expect("tempdir");
    let overlay_dir = dir.path().join("never-published");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir");
    assert_eq!(read_sidecar(&overlay_dir).expect("read"), None);
}

#[test]
fn write_creates_missing_parent_dir() {
    // Write_sidecar must create the overlay_dir if it does not exist —
    // defensive for the install path which targets a not-yet-staged folder.
    let dir = tempdir().expect("tempdir");
    let overlay_dir = dir.path().join("does-not-exist-yet");
    let s = sample();
    write_sidecar(&overlay_dir, &s).expect("write_sidecar must mkdir");
    assert!(overlay_dir.join(SIDECAR_FILENAME).exists());
}

#[test]
fn theme_sidecar_uses_publish_json_suffix() {
    let dir = tempdir().expect("tempdir");
    let themes_dir = dir.path().join("themes");
    let s = sample();
    write_theme_sidecar(&themes_dir, "dark.css", &s).expect("write");
    let p = themes_dir.join(format!("dark.css{THEME_SIDECAR_SUFFIX}"));
    assert!(p.exists(), "expected sidecar at {}", p.display());
    let back = read_theme_sidecar(&themes_dir, "dark.css").expect("read");
    assert_eq!(back, Some(s));
}

#[test]
fn malformed_sidecar_surfaces_invalid_data() {
    let dir = tempdir().expect("tempdir");
    let overlay_dir = dir.path().join("broken");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir");
    std::fs::write(overlay_dir.join(SIDECAR_FILENAME), b"\x00not-json").expect("write");
    let err = read_sidecar(&overlay_dir).expect_err("malformed sidecar must error");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn sidecar_filename_is_dotfile() {
    // Required so `walk_bundle`'s dotfile filter excludes the sidecar from
    // upload bundles automatically (spec §8.1 last paragraph).
    assert!(SIDECAR_FILENAME.starts_with('.'));
}
