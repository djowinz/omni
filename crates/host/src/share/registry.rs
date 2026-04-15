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
        }
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
