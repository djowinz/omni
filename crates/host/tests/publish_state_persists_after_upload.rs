//! End-to-end check that the host's `publish_state::persist_publish_state`
//! helper writes both the per-overlay sidecar AND the workspace-global
//! publish-index after a successful publish.
//!
//! Why this test pins the boundary: the gap that survived upload-flow
//! Wave A was that the helper functions (`write_sidecar`, `upsert`)
//! existed and had unit tests, but no production caller fired them in
//! `handle_publish`'s success path. OWI-46 mocked
//! `workspace.listPublishables` to return hand-staged sidecars, so the
//! test never exercised the writer. This test fixes that gap by calling
//! the helper directly with realistic inputs and asserting both the
//! sidecar file AND a publish-index row appear on disk.
//!
//! Full WS-handler coverage (handle_publish → upload → persist) lives in
//! `share_upload_integration.rs` and is `#[ignore]`-gated on Ultralight
//! resources; this test runs unconditionally in `cargo test -p host` to
//! catch the regression class that produced the original bug.

use omni_host::share::publish_state::{persist_publish_state, PersistInputs};
use omni_host::share::sidecar::read_sidecar;
use omni_host::share::upload::ArtifactKind;
use tempfile::tempdir;

#[test]
fn publish_writes_sidecar_with_full_manifest_fields() {
    let tmp = tempdir().expect("tempdir");
    let overlay_dir = tmp.path().join("overlays").join("marathon-hud");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir");

    let tags = vec!["marathon".to_string(), "running".to_string()];
    let index_path = tmp.path().join("publish-index.json");
    persist_publish_state(&PersistInputs {
        data_dir: tmp.path(),
        kind: ArtifactKind::Bundle,
        workspace_path: "overlays/marathon-hud",
        pubkey_hex: "abcd0123",
        artifact_id: "ov_01J8XKZ",
        version: "1.2.0",
        description: "marathon HUD with splits",
        tags: &tags,
        license: "MIT",
        index_path: Some(&index_path),
    });

    let sidecar = read_sidecar(&overlay_dir).expect("read").expect("Some");
    assert_eq!(sidecar.artifact_id, "ov_01J8XKZ");
    assert_eq!(sidecar.author_pubkey_hex, "abcd0123");
    assert_eq!(sidecar.version, "1.2.0");
    assert_eq!(sidecar.description, "marathon HUD with splits");
    assert_eq!(
        sidecar.tags,
        vec!["marathon".to_string(), "running".to_string()]
    );
    assert_eq!(sidecar.license, "MIT");
    // RFC 3339 timestamp should be ~now, not empty.
    assert!(sidecar.last_published_at.starts_with("20"));
    assert!(sidecar.last_published_at.ends_with('Z'));
}

#[test]
fn publish_overwrites_existing_sidecar_on_update() {
    // Update flow: an existing sidecar from a prior publish gets replaced
    // with the new artifact_id + version. The renderer's `detectMode`
    // looks at the current pubkey vs sidecar's, so persist_publish_state
    // must be free to overwrite without merge logic.
    let tmp = tempdir().expect("tempdir");
    let overlay_dir = tmp.path().join("overlays").join("marathon-hud");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir");
    let empty: Vec<String> = Vec::new();
    let index_path = tmp.path().join("publish-index.json");
    persist_publish_state(&PersistInputs {
        data_dir: tmp.path(),
        kind: ArtifactKind::Bundle,
        workspace_path: "overlays/marathon-hud",
        pubkey_hex: "abcd",
        artifact_id: "ov_first",
        version: "1.0.0",
        description: "first",
        tags: &empty,
        license: "MIT",
        index_path: Some(&index_path),
    });
    let new_tags = vec!["new".to_string()];
    persist_publish_state(&PersistInputs {
        data_dir: tmp.path(),
        kind: ArtifactKind::Bundle,
        workspace_path: "overlays/marathon-hud",
        pubkey_hex: "abcd",
        artifact_id: "ov_second",
        version: "1.1.0",
        description: "second",
        tags: &new_tags,
        license: "Apache-2.0",
        index_path: Some(&index_path),
    });
    let sidecar = read_sidecar(&overlay_dir).expect("read").expect("Some");
    assert_eq!(sidecar.artifact_id, "ov_second");
    assert_eq!(sidecar.version, "1.1.0");
    assert_eq!(sidecar.license, "Apache-2.0");
    assert_eq!(sidecar.tags, vec!["new".to_string()]);
}

#[test]
fn publish_upserts_publish_index_row_at_overridden_path() {
    let tmp = tempdir().expect("tempdir");
    let overlay_dir = tmp.path().join("overlays").join("marathon-hud");
    std::fs::create_dir_all(&overlay_dir).expect("mkdir");
    let index_path = tmp.path().join("publish-index.json");

    persist_publish_state(&PersistInputs {
        data_dir: tmp.path(),
        kind: ArtifactKind::Bundle,
        workspace_path: "overlays/marathon-hud",
        pubkey_hex: "abcd",
        artifact_id: "ov_first",
        version: "1.0.0",
        description: "",
        tags: &[],
        license: "MIT",
        index_path: Some(&index_path),
    });

    let idx = omni_host::share::publish_index::read(&index_path).expect("read");
    assert_eq!(idx.entries.len(), 1);
    let row = &idx.entries[0];
    assert_eq!(row.pubkey_hex, "abcd");
    assert_eq!(row.kind, "overlay");
    assert_eq!(row.name, "marathon-hud");
    assert_eq!(row.artifact_id, "ov_first");
    assert_eq!(row.last_version, "1.0.0");
}
