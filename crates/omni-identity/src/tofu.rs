//! Trust-On-First-Use registry. Maps author pubkey → display_name + metadata.
//! Detects display-name claims from two different pubkeys (impersonation signal).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::atomic::atomic_write;
use crate::error::IdentityError;
use crate::fingerprint::PublicKey;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TofuResult {
    FirstSeen,
    KnownMatch,
    DisplayNameMismatch { known_pubkey_hex: String, seen_pubkey_hex: String, display_name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TofuEntry {
    pub first_seen_at: u64,
    pub display_name: String,
    pub fingerprint_words: String,
    pub install_count: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TofuDoc {
    version: u32,
    /// Keyed by lowercase hex of the 32-byte pubkey.
    entries: HashMap<String, TofuEntry>,
}

#[derive(Debug)]
pub struct TofuRegistry {
    path: PathBuf,
    doc: TofuDoc,
}

impl TofuRegistry {
    pub fn load(path: &Path) -> Result<Self, IdentityError> {
        let doc = if path.exists() {
            let bytes = std::fs::read(path)?;
            let doc: TofuDoc = serde_json::from_slice(&bytes)
                .map_err(|e| IdentityError::Tofu(format!("parse: {e}")))?;
            if doc.version != 1 {
                return Err(IdentityError::UnsupportedVersion(doc.version as u8));
            }
            doc
        } else {
            TofuDoc { version: 1, entries: HashMap::new() }
        };
        Ok(Self { path: path.to_path_buf(), doc })
    }

    pub fn save(&self) -> Result<(), IdentityError> {
        let bytes = serde_json::to_vec_pretty(&self.doc)
            .map_err(|e| IdentityError::Tofu(format!("serialize: {e}")))?;
        atomic_write(&self.path, &bytes)
    }

    pub fn check_or_record(
        &mut self,
        pubkey: PublicKey,
        display_name: &str,
        now_unix: u64,
    ) -> TofuResult {
        let pk_hex = pubkey.to_hex();

        // Impersonation check: same display_name, different pubkey?
        for (other_hex, entry) in self.doc.entries.iter() {
            if other_hex != &pk_hex && entry.display_name == display_name {
                return TofuResult::DisplayNameMismatch {
                    known_pubkey_hex: other_hex.clone(),
                    seen_pubkey_hex: pk_hex,
                    display_name: display_name.to_string(),
                };
            }
        }

        if self.doc.entries.contains_key(&pk_hex) {
            TofuResult::KnownMatch
        } else {
            let fp = pubkey.fingerprint();
            let words = fp.to_words();
            self.doc.entries.insert(
                pk_hex,
                TofuEntry {
                    first_seen_at: now_unix,
                    display_name: display_name.to_string(),
                    fingerprint_words: format!("{}-{}-{}", words[0], words[1], words[2]),
                    install_count: 0,
                },
            );
            TofuResult::FirstSeen
        }
    }

    pub fn record_install(&mut self, pubkey: PublicKey) {
        let pk_hex = pubkey.to_hex();
        if let Some(e) = self.doc.entries.get_mut(&pk_hex) {
            e.install_count = e.install_count.saturating_add(1);
        }
    }

    pub fn entry(&self, pubkey: PublicKey) -> Option<&TofuEntry> {
        self.doc.entries.get(&pubkey.to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn pk(seed: u8) -> PublicKey { PublicKey([seed; 32]) }

    #[test]
    fn first_seen_then_known_match() {
        let dir = tempdir().unwrap();
        let mut r = TofuRegistry::load(&dir.path().join("tofu.json")).unwrap();
        assert_eq!(r.check_or_record(pk(1), "alice", 100), TofuResult::FirstSeen);
        assert_eq!(r.check_or_record(pk(1), "alice", 200), TofuResult::KnownMatch);
    }

    #[test]
    fn display_name_mismatch_detected() {
        let dir = tempdir().unwrap();
        let mut r = TofuRegistry::load(&dir.path().join("tofu.json")).unwrap();
        r.check_or_record(pk(1), "alice", 100);
        match r.check_or_record(pk(2), "alice", 200) {
            TofuResult::DisplayNameMismatch { display_name, .. } => {
                assert_eq!(display_name, "alice");
            }
            other => panic!("expected mismatch, got {other:?}"),
        }
    }

    #[test]
    fn persists_to_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tofu.json");
        {
            let mut r = TofuRegistry::load(&path).unwrap();
            r.check_or_record(pk(7), "bob", 500);
            r.record_install(pk(7));
            r.record_install(pk(7));
            r.save().unwrap();
        }
        let r2 = TofuRegistry::load(&path).unwrap();
        let e = r2.entry(pk(7)).unwrap();
        assert_eq!(e.display_name, "bob");
        assert_eq!(e.install_count, 2);
    }

    #[test]
    fn rejects_unsupported_version() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("tofu.json");
        std::fs::write(&p, br#"{"version": 99, "entries": {}}"#).unwrap();
        let err = TofuRegistry::load(&p).unwrap_err();
        assert!(matches!(err, IdentityError::UnsupportedVersion(99)));
    }

    #[test]
    fn record_install_noop_for_unknown_pubkey() {
        let dir = tempdir().unwrap();
        let mut r = TofuRegistry::load(&dir.path().join("tofu.json")).unwrap();
        r.record_install(pk(42)); // no-op
        assert!(r.entry(pk(42)).is_none());
    }
}
