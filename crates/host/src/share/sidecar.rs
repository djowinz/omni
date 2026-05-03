//! Per-overlay/per-theme `.omni-publish.json` sidecar I/O.
//!
//! Spec: upload-flow-redesign §8.1 / INV-7.6.1 / INV-7.6.4. Written on every
//! successful publish/update/install; read by the upload-dialog source picker
//! to detect "this artifact has been published before" and switch the dialog
//! into update-mode. Authority for "did THIS identity publish this?" is the
//! `author_pubkey_hex` field — a sidecar whose pubkey doesn't match the
//! current identity surfaces the new-artifact warning banner instead.
//!
//! Sidecar layout (overlays):
//!   `overlays/<name>/.omni-publish.json` — dotfile, naturally excluded from
//!   the upload bundle by `walk_bundle`'s dotfile filter (spec §8.1 last
//!   paragraph).
//!
//! Sidecar layout (themes):
//!   `themes/<name>.publish.json` — flat sibling next to the theme CSS file
//!   (themes are single files, not directories). The worker only reads the
//!   theme CSS itself, so the flat sidecar is naturally out-of-band.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// On-disk shape of `.omni-publish.json`.
///
/// Field semantics:
/// - `artifact_id` — the worker's stable artifact id (e.g. `ov_01J8XKZ...`).
///   This is what re-upload requests address; missing it forces a new
///   artifact creation.
/// - `author_pubkey_hex` — the pubkey of the identity that performed the
///   most recent publish/update. On install, this is the ORIGINAL author's
///   pubkey (not the installer's), so `==` against the local identity is the
///   "did I publish this?" oracle.
/// - `version` — the most recently published semver (string-encoded so the
///   sidecar tolerates whatever format the worker returns).
/// - `last_published_at` — RFC 3339 timestamp string, used in UI banners.
///
/// Wire-typed across the editor boundary: `workspace.listPublishables` returns
/// a `PublishablesEntry` (`crates/host/src/share/ws_messages.rs`) whose
/// `sidecar: Option<PublishSidecar>` field surfaces this struct verbatim to
/// the renderer. The `ts_rs::TS` derive emits the matching TypeScript view to
/// `packages/shared-types/src/generated/PublishSidecar.ts`. Any field added
/// here must keep `serde(default)` semantics for forward-compat with sidecars
/// written by older host versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct PublishSidecar {
    pub artifact_id: String,
    pub author_pubkey_hex: String,
    pub version: String,
    pub last_published_at: String,
    /// Last-published manifest description. Cached locally so the upload
    /// dialog's Step 2 form (INV-7.5.3) can prefill on update mode without
    /// a worker round-trip. `#[serde(default)]` keeps older sidecars
    /// (written before this field existed) deserializing cleanly.
    #[serde(default)]
    pub description: String,
    /// Last-published manifest tag list. Cached locally for INV-7.5.3
    /// prefill. See `description` for the `serde(default)` rationale.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Last-published manifest license string (SPDX or free-form Custom).
    /// Cached locally for INV-7.5.3 prefill. See `description`.
    #[serde(default)]
    pub license: String,
}

/// Filename used for overlay sidecars (dotfile so `walk_bundle` skips it).
pub const SIDECAR_FILENAME: &str = ".omni-publish.json";

/// Suffix used for theme sidecars: `<themename>.css.publish.json`.
///
/// Themes are single files, so the sidecar lives flat alongside them rather
/// than inside a per-artifact directory.
pub const THEME_SIDECAR_SUFFIX: &str = ".publish.json";

/// Read a sidecar from an overlay directory. Returns `Ok(None)` when the file
/// does not exist (the common case for never-published overlays). Malformed
/// JSON surfaces as `io::ErrorKind::InvalidData`.
pub fn read_sidecar(overlay_dir: &Path) -> std::io::Result<Option<PublishSidecar>> {
    let path = overlay_dir.join(SIDECAR_FILENAME);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let parsed = serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(Some(parsed))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Write (or overwrite) the sidecar in `overlay_dir`. Creates the directory if
/// it does not exist (defensive — callers from the install path may target a
/// not-yet-staged overlay folder).
pub fn write_sidecar(overlay_dir: &Path, sidecar: &PublishSidecar) -> std::io::Result<()> {
    std::fs::create_dir_all(overlay_dir)?;
    let path = overlay_dir.join(SIDECAR_FILENAME);
    let bytes = serde_json::to_vec_pretty(sidecar)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, bytes)
}

/// Read a theme sidecar. `theme_filename` is the bare CSS filename (e.g.
/// `dark.css`); the sidecar lives at `themes_dir/<theme_filename>.publish.json`.
pub fn read_theme_sidecar(
    themes_dir: &Path,
    theme_filename: &str,
) -> std::io::Result<Option<PublishSidecar>> {
    let path = themes_dir.join(format!("{theme_filename}{THEME_SIDECAR_SUFFIX}"));
    match std::fs::read(&path) {
        Ok(bytes) => {
            let parsed = serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(Some(parsed))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Write a theme sidecar. Mirrors `write_sidecar` for overlays.
pub fn write_theme_sidecar(
    themes_dir: &Path,
    theme_filename: &str,
    sidecar: &PublishSidecar,
) -> std::io::Result<()> {
    std::fs::create_dir_all(themes_dir)?;
    let path = themes_dir.join(format!("{theme_filename}{THEME_SIDECAR_SUFFIX}"));
    let bytes = serde_json::to_vec_pretty(sidecar)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample() -> PublishSidecar {
        PublishSidecar {
            artifact_id: "ov_01J8XKZ".into(),
            author_pubkey_hex: "abcd".into(),
            version: "1.3.0".into(),
            last_published_at: "2026-04-18T18:12:44Z".into(),
            description: "marathon run HUD".into(),
            tags: vec!["marathon".into(), "running".into()],
            license: "MIT".into(),
        }
    }

    #[test]
    fn json_roundtrip_preserves_fields() {
        let s = sample();
        let bytes = serde_json::to_vec(&s).unwrap();
        let back: PublishSidecar = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn read_returns_none_when_dir_empty() {
        let dir = tempdir().unwrap();
        assert_eq!(read_sidecar(dir.path()).unwrap(), None);
    }

    #[test]
    fn write_then_read_overlay_roundtrip() {
        let dir = tempdir().unwrap();
        let overlay_dir = dir.path().join("overlays").join("marathon-hud");
        let s = sample();
        write_sidecar(&overlay_dir, &s).unwrap();
        let back = read_sidecar(&overlay_dir).unwrap();
        assert_eq!(back, Some(s));
    }

    #[test]
    fn write_then_read_theme_roundtrip() {
        let dir = tempdir().unwrap();
        let themes_dir = dir.path().join("themes");
        let s = sample();
        write_theme_sidecar(&themes_dir, "dark.css", &s).unwrap();
        let back = read_theme_sidecar(&themes_dir, "dark.css").unwrap();
        assert_eq!(back, Some(s));
    }

    #[test]
    fn malformed_json_returns_invalid_data() {
        let dir = tempdir().unwrap();
        let overlay_dir = dir.path().join("overlays").join("broken");
        std::fs::create_dir_all(&overlay_dir).unwrap();
        std::fs::write(overlay_dir.join(SIDECAR_FILENAME), b"not json").unwrap();
        let err = read_sidecar(&overlay_dir).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn deserializes_old_shape_sidecar_without_new_fields() {
        // Sidecars written before the description/tags/license expansion must
        // still load cleanly — `serde(default)` provides empty defaults.
        let dir = tempdir().unwrap();
        let overlay_dir = dir.path().join("overlays").join("legacy");
        std::fs::create_dir_all(&overlay_dir).unwrap();
        let old_shape = br#"{
            "artifact_id": "ov_legacy",
            "author_pubkey_hex": "abcd",
            "version": "0.1.0",
            "last_published_at": "2026-04-18T00:00:00Z"
        }"#;
        std::fs::write(overlay_dir.join(SIDECAR_FILENAME), old_shape).unwrap();
        let back = read_sidecar(&overlay_dir).unwrap().expect("Some");
        assert_eq!(back.artifact_id, "ov_legacy");
        assert_eq!(back.description, "");
        assert!(back.tags.is_empty());
        assert_eq!(back.license, "");
    }
}
