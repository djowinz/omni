//! Installed-artifact registries written at `%APPDATA%\Omni\`:
//!   - `installed-themes.json`
//!   - `installed-bundles.json`
//!
//! Schema is versioned (`version: u32`, starts at 1). Reads are plain
//! `serde_json::from_reader`; writes use an adjacent `.tmp` + rename via the
//! shared `AtomicDir` to make updates crash-safe.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegistryError {
    #[error("io failure: {0}")]
    Io(#[from] io::Error),
    #[error("schema decode failure: {0}")]
    Decode(#[source] serde_json::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryKind { Themes, Bundles }

impl RegistryKind {
    fn filename(self) -> &'static str {
        match self {
            RegistryKind::Themes => "installed-themes.json",
            RegistryKind::Bundles => "installed-bundles.json",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstalledRegistry {
    pub version: u32,
    pub entries: BTreeMap<String, InstalledEntry>,
}

impl Default for InstalledRegistry {
    fn default() -> Self {
        Self { version: 1, entries: BTreeMap::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledEntry {
    pub artifact_id: String,
    pub content_hash: String,
    pub author_pubkey: String,
    pub fingerprint_hex: String,
    pub source_url: String,
    pub installed_at: u64,
    pub installed_version: semver::Version,
    pub omni_min_version: semver::Version,
    /// Absolute on-disk location of the installed artifact directory.
    ///
    /// Stored explicitly rather than reconstructed from the registry key +
    /// a workspace-root convention: the bundle key format is
    /// `"<pubkey8>-<display_name>"` and hyphens in display names would make
    /// key-parsing ambiguous; deriving the path by convention from the
    /// caller-supplied `target_path` is an uncodified invariant future
    /// install callers could break silently. Cross-sub-spec integration for
    /// #013 fork-to-local, which needs key → on-disk path resolution.
    #[serde(default)]
    pub installed_path: PathBuf,
    /// User-visible name (as signed in the manifest) preserved on the entry
    /// so consumers do not have to parse it back out of the registry key.
    /// Paired with `installed_path` as the #010/#013 integration addition.
    #[serde(default)]
    pub display_name: String,
}

pub struct RegistryHandle {
    path: PathBuf,
    data: InstalledRegistry,
}

impl RegistryHandle {
    pub fn load(app_data_dir: &Path, kind: RegistryKind) -> Result<Self, RegistryError> {
        let path = app_data_dir.join(kind.filename());
        let data = if path.exists() {
            let bytes = fs::read(&path)?;
            serde_json::from_slice(&bytes).map_err(RegistryError::Decode)?
        } else {
            InstalledRegistry::default()
        };
        Ok(Self { path, data })
    }

    pub fn entries(&self) -> &BTreeMap<String, InstalledEntry> {
        &self.data.entries
    }

    pub fn upsert(&mut self, key: String, entry: InstalledEntry) {
        self.data.entries.insert(key, entry);
    }

    /// Resolve a bundle registry key to its entry. Added for #013 fork-to-local;
    /// `lookup_theme` mirrors the shape so the next theme-registry consumer
    /// does not add a third ad-hoc accessor.
    pub fn lookup_bundle(&self, key: &str) -> Option<&InstalledEntry> {
        self.data.entries.get(key)
    }

    pub fn lookup_theme(&self, key: &str) -> Option<&InstalledEntry> {
        self.data.entries.get(key)
    }

    /// Atomic write via `<path>.tmp` + rename. (A full AtomicDir is overkill
    /// for a single file; a sibling temp + rename achieves the same guarantee
    /// — used here to keep registry writes independent of AtomicDir's
    /// directory-oriented API.)
    pub fn save(&self) -> Result<(), RegistryError> {
        let bytes = serde_json::to_vec_pretty(&self.data).map_err(RegistryError::Decode)?;
        let tmp = self.path.with_extension("json.tmp");
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&tmp, &bytes)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_entry() -> InstalledEntry {
        InstalledEntry {
            artifact_id: "abc".into(),
            content_hash: "deadbeef".into(),
            author_pubkey: "00".repeat(32),
            fingerprint_hex: "112233445566".into(),
            source_url: "https://worker.example/v1/download/abc".into(),
            installed_at: 1_700_000_000,
            installed_version: semver::Version::new(1, 0, 0),
            omni_min_version: semver::Version::new(0, 1, 0),
            installed_path: PathBuf::from("/tmp/abc"),
            display_name: "abc".into(),
        }
    }

    #[test]
    fn lookup_bundle_returns_entry() {
        let dir = TempDir::new().unwrap();
        let mut r = RegistryHandle::load(dir.path(), RegistryKind::Bundles).unwrap();
        r.upsert("deadbeef-abc".into(), sample_entry());
        assert_eq!(r.lookup_bundle("deadbeef-abc").unwrap().display_name, "abc");
        assert!(r.lookup_bundle("missing").is_none());
    }

    #[test]
    fn legacy_entry_without_new_fields_loads_with_serde_default() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("installed-bundles.json");
        std::fs::write(&path, br#"{
            "version": 1,
            "entries": {
                "deadbeef-legacy": {
                    "artifact_id": "legacy",
                    "content_hash": "ff",
                    "author_pubkey": "00",
                    "fingerprint_hex": "11",
                    "source_url": "download://legacy",
                    "installed_at": 1,
                    "installed_version": "1.0.0",
                    "omni_min_version": "0.1.0"
                }
            }
        }"#).unwrap();
        let r = RegistryHandle::load(dir.path(), RegistryKind::Bundles).unwrap();
        let e = r.lookup_bundle("deadbeef-legacy").unwrap();
        assert_eq!(e.installed_path, PathBuf::new());
        assert_eq!(e.display_name, "");
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = TempDir::new().unwrap();
        let r = RegistryHandle::load(dir.path(), RegistryKind::Themes).unwrap();
        assert_eq!(r.data.version, 1);
        assert!(r.entries().is_empty());
    }

    #[test]
    fn upsert_save_reload_roundtrip() {
        let dir = TempDir::new().unwrap();
        {
            let mut r = RegistryHandle::load(dir.path(), RegistryKind::Bundles).unwrap();
            r.upsert("author-slug-name".into(), sample_entry());
            r.save().unwrap();
        }
        let r = RegistryHandle::load(dir.path(), RegistryKind::Bundles).unwrap();
        assert_eq!(r.entries().len(), 1);
        assert_eq!(r.entries().get("author-slug-name").unwrap().artifact_id, "abc");
    }

    #[test]
    fn save_leaves_no_tmp_file_on_success() {
        let dir = TempDir::new().unwrap();
        let mut r = RegistryHandle::load(dir.path(), RegistryKind::Themes).unwrap();
        r.upsert("x".into(), sample_entry());
        r.save().unwrap();
        assert!(dir.path().join("installed-themes.json").exists());
        assert!(!dir.path().join("installed-themes.json.tmp").exists());
    }
}
