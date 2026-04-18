//! Adapter over `identity::TofuRegistry` for the install pipeline.
//!
//! Responsibilities: load/save the registry at the canonical path, expose
//! `check_or_record` + `record_install` wrappers, format pubkey/fingerprint
//! consistently for error messages (Display per invariant #20).

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use identity::{IdentityError, PublicKey, TofuRegistry, TofuResult};

pub struct TofuStore {
    inner: TofuRegistry,
    path: PathBuf,
}

impl TofuStore {
    pub fn open(app_data_dir: &Path) -> Result<Self, IdentityError> {
        let path = app_data_dir.join("tofu-fingerprints.json");
        let inner = TofuRegistry::load(&path)?;
        Ok(Self { inner, path })
    }

    pub fn check_or_record(
        &mut self,
        pubkey: &PublicKey,
        display_name: &str,
    ) -> Result<TofuResult, IdentityError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        Ok(self.inner.check_or_record(*pubkey, display_name, now))
    }

    pub fn record_install(&mut self, pubkey: &PublicKey) -> Result<(), IdentityError> {
        self.inner.record_install(*pubkey);
        self.inner.save()
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_with_missing_file_returns_empty_registry() {
        let dir = TempDir::new().unwrap();
        let store = TofuStore::open(dir.path()).unwrap();
        assert_eq!(store.path(), &*dir.path().join("tofu-fingerprints.json"));
    }
}
