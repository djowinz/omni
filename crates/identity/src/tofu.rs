//! Trust-On-First-Use registry. Maps author pubkey → display_name + metadata.
//!
//! Per 2026-04-26 identity-completion-and-display-name spec §2: the prior
//! impersonation check (alarming on display_name reuse across pubkeys) was
//! removed because display_names are non-unique by design — the pubkey-slice
//! is the canonical disambiguator (`<display_name>#<8-hex>`). The registry
//! now stores `Option<String>` for display_name so callers without a label
//! (e.g., when the worker resolver is offline) record `None` rather than a
//! placeholder.

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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TofuEntry {
    pub first_seen_at: u64,
    pub display_name: Option<String>,
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
                return Err(IdentityError::Tofu(format!(
                    "unsupported version: {}",
                    doc.version
                )));
            }
            doc
        } else {
            TofuDoc {
                version: 1,
                entries: HashMap::new(),
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            doc,
        })
    }

    pub fn save(&self) -> Result<(), IdentityError> {
        let bytes = serde_json::to_vec_pretty(&self.doc)
            .map_err(|e| IdentityError::Tofu(format!("serialize: {e}")))?;
        atomic_write(&self.path, &bytes)
    }

    /// Look up `pubkey`. If unseen, record it with `display_name` (which may
    /// be `None` when the worker resolver is offline — better to label nothing
    /// than to label wrong) and return `FirstSeen`. If already known, return
    /// `KnownMatch`. Persists the registry on first-seen.
    ///
    /// Per 2026-04-26 spec §2: the prior impersonation check was removed —
    /// display_names are non-unique under the `<display_name>#<8-hex>`
    /// disambiguation scheme, so the check returned false positives by design.
    pub fn check_or_record(
        &mut self,
        pubkey: PublicKey,
        display_name: Option<&str>,
        now_unix: u64,
    ) -> Result<TofuResult, IdentityError> {
        let pk_hex = pubkey.to_hex();
        use std::collections::hash_map::Entry;
        let result = match self.doc.entries.entry(pk_hex) {
            Entry::Occupied(_) => TofuResult::KnownMatch,
            Entry::Vacant(e) => {
                let fp = pubkey.fingerprint();
                let words = fp.to_words();
                e.insert(TofuEntry {
                    first_seen_at: now_unix,
                    display_name: display_name.map(|s| s.to_string()),
                    fingerprint_words: format!("{}-{}-{}", words[0], words[1], words[2]),
                    install_count: 0,
                });
                TofuResult::FirstSeen
            }
        };
        self.save()?;
        Ok(result)
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

    fn pk(seed: u8) -> PublicKey {
        PublicKey([seed; 32])
    }

    #[test]
    fn first_seen_then_known_match() {
        let dir = tempdir().unwrap();
        let mut r = TofuRegistry::load(&dir.path().join("tofu.json")).unwrap();
        assert_eq!(
            r.check_or_record(pk(1), Some("alice"), 100).unwrap(),
            TofuResult::FirstSeen
        );
        assert_eq!(
            r.check_or_record(pk(1), Some("alice"), 200).unwrap(),
            TofuResult::KnownMatch
        );
    }

    #[test]
    fn check_or_record_accepts_none_label() {
        let dir = tempdir().unwrap();
        let mut r = TofuRegistry::load(&dir.path().join("tofu.json")).unwrap();
        let result = r.check_or_record(pk(1), None, 100);
        assert!(matches!(result, Ok(TofuResult::FirstSeen)));

        let entry = r.entry(pk(1)).unwrap();
        assert_eq!(entry.display_name, None);
    }

    #[test]
    fn check_or_record_does_not_emit_display_name_mismatch() {
        // Two pubkeys with the same display_name no longer trip an alarm:
        // display_names are non-unique in v1, the slice is the disambiguator.
        // (Per 2026-04-26 identity-completion-and-display-name spec §2.)
        let dir = tempdir().unwrap();
        let mut r = TofuRegistry::load(&dir.path().join("tofu.json")).unwrap();
        r.check_or_record(pk(1), Some("alice"), 100).unwrap();
        let result = r.check_or_record(pk(2), Some("alice"), 200).unwrap();
        assert!(
            matches!(result, TofuResult::FirstSeen),
            "expected FirstSeen for new pubkey regardless of display_name collision, got {result:?}"
        );
    }

    #[test]
    fn persists_to_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tofu.json");
        {
            let mut r = TofuRegistry::load(&path).unwrap();
            r.check_or_record(pk(7), Some("bob"), 500).unwrap();
            r.record_install(pk(7));
            r.record_install(pk(7));
            r.save().unwrap();
        }
        let r2 = TofuRegistry::load(&path).unwrap();
        let e = r2.entry(pk(7)).unwrap();
        assert_eq!(e.display_name.as_deref(), Some("bob"));
        assert_eq!(e.install_count, 2);
    }

    #[test]
    fn rejects_unsupported_version() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("tofu.json");
        std::fs::write(&p, br#"{"version": 99, "entries": {}}"#).unwrap();
        let err = TofuRegistry::load(&p).unwrap_err();
        assert!(matches!(err, IdentityError::Tofu(_)));
    }

    #[test]
    fn record_install_noop_for_unknown_pubkey() {
        let dir = tempdir().unwrap();
        let mut r = TofuRegistry::load(&dir.path().join("tofu.json")).unwrap();
        r.record_install(pk(42)); // no-op
        assert!(r.entry(pk(42)).is_none());
    }
}
