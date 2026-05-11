//! Workspace-global publish-index — silent-restore source for missing
//! `.omni-publish.json` sidecars.
//!
//! Spec: upload-flow-redesign §8.2 / INV-7.6.1. Lives at
//! `%APPDATA%\Omni\publish-index.json`. Updated on every publish/update/install
//! so that even if the per-overlay sidecar is deleted (e.g. by a manual
//! workspace reset, app reinstall pre-AppData wipe), the upload dialog can
//! reconstruct "this overlay-name has been published before by my identity"
//! without an extra worker round-trip.
//!
//! Lookup key tuple: `(pubkey_hex, kind, name)`. Different identities can
//! publish overlays with the same name (the server enforces uniqueness only
//! per-author), so the pubkey is part of the index key.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One row in the publish-index. `kind` is the closed vocabulary
/// `"overlay" | "theme"` — kept as a `String` here (rather than an enum) so
/// the JSON file forward-compatibly tolerates unknown values from a future
/// host release without round-trip data loss.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishIndexEntry {
    pub pubkey_hex: String,
    pub kind: String,
    pub name: String,
    pub artifact_id: String,
    pub last_version: String,
    pub last_published_at: String,
}

/// On-disk shape of the publish-index file. Single `entries` array keyed
/// linearly; the workspace contains O(10s) of artifacts, so a list is fine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublishIndex {
    pub entries: Vec<PublishIndexEntry>,
}

impl PublishIndex {
    /// Find an entry by (pubkey, kind, name) tuple. Returns `None` when no
    /// match exists; the caller surfaces this as "first publish under this
    /// identity for this name."
    pub fn lookup(&self, pubkey_hex: &str, kind: &str, name: &str) -> Option<&PublishIndexEntry> {
        self.entries
            .iter()
            .find(|e| e.pubkey_hex == pubkey_hex && e.kind == kind && e.name == name)
    }

    /// Insert or replace an entry (matched by the (pubkey, kind, name) tuple).
    pub fn upsert(&mut self, entry: PublishIndexEntry) {
        if let Some(existing) = self.entries.iter_mut().find(|e| {
            e.pubkey_hex == entry.pubkey_hex && e.kind == entry.kind && e.name == entry.name
        }) {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
    }

    /// Remove an entry by (pubkey, kind, name). Returns `true` if a row was
    /// removed. Provided for future "rotate identity" / "delete artifact"
    /// flows; not exercised by Wave A0.
    pub fn remove(&mut self, pubkey_hex: &str, kind: &str, name: &str) -> bool {
        let before = self.entries.len();
        self.entries
            .retain(|e| !(e.pubkey_hex == pubkey_hex && e.kind == kind && e.name == name));
        self.entries.len() != before
    }

    /// Find an entry by `artifact_id`. Used by the `publish.lookupWorkspace`
    /// WS handler to resolve a my-uploads card's artifact_id to its local
    /// workspace folder for the Update CTA.
    ///
    /// Linear scan — index size is bounded by per-author publish count
    /// (typically O(10s)), matching the comment on `PublishIndex` itself.
    pub fn lookup_by_artifact_id(&self, artifact_id: &str) -> Option<&PublishIndexEntry> {
        self.entries.iter().find(|e| e.artifact_id == artifact_id)
    }
}

pub const INDEX_FILENAME: &str = "publish-index.json";

/// Default location of the publish-index: `%APPDATA%\Omni\publish-index.json`.
///
/// Reuses [`crate::config::data_dir`] (the established pattern; tofu and
/// registry both use it) instead of pulling in the `directories` crate.
pub fn index_path() -> PathBuf {
    crate::config::data_dir().join(INDEX_FILENAME)
}

/// Read the publish-index from `path`. Missing file → empty index. Malformed
/// JSON also surfaces as empty (rather than errored) — the index is a derived
/// cache and must never block dialog open.
pub fn read(path: &Path) -> std::io::Result<PublishIndex> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PublishIndex::default()),
        Err(e) => Err(e),
    }
}

/// Write the publish-index. Creates the parent directory if needed.
pub fn write(path: &Path, idx: &PublishIndex) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(idx)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn lookup_misses_with_different_kind() {
        let mut idx = PublishIndex::default();
        idx.upsert(entry("marathon", "1.0.0"));
        assert!(idx.lookup("ab", "theme", "marathon").is_none());
    }

    #[test]
    fn upsert_replaces_existing_entry() {
        let mut idx = PublishIndex::default();
        idx.upsert(entry("marathon", "1.0.0"));
        idx.upsert(entry("marathon", "1.1.0"));
        assert_eq!(idx.entries.len(), 1);
        assert_eq!(
            idx.lookup("ab", "overlay", "marathon")
                .unwrap()
                .last_version,
            "1.1.0"
        );
    }

    #[test]
    fn remove_deletes_matching_entry() {
        let mut idx = PublishIndex::default();
        idx.upsert(entry("marathon", "1.0.0"));
        assert!(idx.remove("ab", "overlay", "marathon"));
        assert!(idx.lookup("ab", "overlay", "marathon").is_none());
    }

    #[test]
    fn lookup_by_artifact_id_returns_matching_entry() {
        let mut idx = PublishIndex::default();
        idx.upsert(PublishIndexEntry {
            pubkey_hex: "ab".into(),
            kind: "overlay".into(),
            name: "marathon".into(),
            artifact_id: "art-1".into(),
            last_version: "1.0.0".into(),
            last_published_at: "2026-04-18T00:00:00Z".into(),
        });
        assert_eq!(idx.lookup_by_artifact_id("art-1").unwrap().name, "marathon");
    }

    #[test]
    fn lookup_by_artifact_id_returns_none_for_unknown() {
        let idx = PublishIndex::default();
        assert!(idx.lookup_by_artifact_id("art-missing").is_none());
    }

    #[test]
    fn lookup_by_artifact_id_returns_first_match_when_duplicates() {
        // Should not happen in practice (artifact_id is server-unique), but
        // confirm the linear scan returns the first match deterministically.
        let mut idx = PublishIndex::default();
        idx.entries.push(PublishIndexEntry {
            pubkey_hex: "ab".into(),
            kind: "overlay".into(),
            name: "first".into(),
            artifact_id: "dup".into(),
            last_version: "1.0.0".into(),
            last_published_at: "2026-04-18T00:00:00Z".into(),
        });
        idx.entries.push(PublishIndexEntry {
            pubkey_hex: "ab".into(),
            kind: "overlay".into(),
            name: "second".into(),
            artifact_id: "dup".into(),
            last_version: "1.0.0".into(),
            last_published_at: "2026-04-18T00:00:00Z".into(),
        });
        assert_eq!(idx.lookup_by_artifact_id("dup").unwrap().name, "first");
    }
}
