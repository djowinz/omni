//! Atomic directory staging helper for workspace install pipeline.
//!
//! Provides [`AtomicDir`] — a same-volume staging directory that commits via
//! `fs::rename` and cleans itself up on drop — and [`sweep_orphans`] for
//! clearing leftover staging directories after a crashed install.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use uuid::Uuid;

const STAGING_PREFIX: &str = ".omni-staging-";

/// RAII guard that removes a staging directory on drop unless disarmed.
struct ManualTempDirGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for ManualTempDirGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

/// Same-volume staging directory that renames into place on commit.
///
/// Dropping without `commit` removes the staging directory. `commit` renames
/// the staging directory onto `final_path` atomically on the same volume.
pub struct AtomicDir {
    temp: ManualTempDirGuard,
    final_path: PathBuf,
}

impl AtomicDir {
    /// Create a staging directory alongside `final_path`'s parent.
    ///
    /// Returns `ErrorKind::InvalidInput` if `final_path` has no parent.
    pub fn stage(final_path: &Path) -> io::Result<Self> {
        let parent = final_path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "final_path has no parent directory",
            )
        })?;
        fs::create_dir_all(parent)?;
        let staging_path = parent.join(format!("{}{}", STAGING_PREFIX, Uuid::new_v4()));
        fs::create_dir(&staging_path)?;
        Ok(Self {
            temp: ManualTempDirGuard {
                path: staging_path,
                armed: true,
            },
            final_path: final_path.to_path_buf(),
        })
    }

    /// Path to the staging directory. Write files here before calling `commit`.
    pub fn path(&self) -> &Path {
        &self.temp.path
    }

    /// Commit the staging directory onto `final_path`.
    ///
    /// With `overwrite = false`, returns `ErrorKind::AlreadyExists` if the
    /// target exists; the staging directory is cleaned up on drop. With
    /// `overwrite = true`, removes the existing target (file or directory)
    /// then renames.
    pub fn commit(mut self, overwrite: bool) -> io::Result<()> {
        if self.final_path.exists() {
            if !overwrite {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "target path already exists",
                ));
            }
            if self.final_path.is_dir() {
                fs::remove_dir_all(&self.final_path)?;
            } else {
                fs::remove_file(&self.final_path)?;
            }
        }
        // Disarm before rename so Drop doesn't try to remove the renamed path.
        self.temp.armed = false;
        fs::rename(&self.temp.path, &self.final_path)?;
        Ok(())
    }

    /// Explicitly discard the staging directory (equivalent to dropping).
    pub fn discard(self) {}
}

/// Remove all top-level `.omni-staging-*` directories in `workspace_root`.
///
/// Returns the number of orphan staging directories removed. Returns `Ok(0)`
/// if the root does not exist. Does not recurse; leaves non-staging entries
/// alone.
pub fn sweep_orphans(workspace_root: &Path) -> io::Result<usize> {
    if !workspace_root.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in fs::read_dir(workspace_root)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let name = match file_name.to_str() {
            Some(s) => s,
            None => continue,
        };
        if !name.starts_with(STAGING_PREFIX) {
            continue;
        }
        let file_type = entry.file_type()?;
        if !file_type.is_dir() {
            continue;
        }
        fs::remove_dir_all(entry.path())?;
        count += 1;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn stage_creates_same_volume_staging_dir() {
        let root = TempDir::new().unwrap();
        let final_path = root.path().join("target");
        let staging = AtomicDir::stage(&final_path).unwrap();
        assert!(staging.path().exists());
        assert!(staging
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with(".omni-staging-"));
        assert_eq!(staging.path().parent(), Some(root.path()));
    }

    #[test]
    fn commit_renames_atomically_when_target_absent() {
        let root = TempDir::new().unwrap();
        let final_path = root.path().join("target");
        let staging = AtomicDir::stage(&final_path).unwrap();
        fs::write(staging.path().join("a.txt"), b"hello").unwrap();
        staging.commit(false).unwrap();
        assert!(final_path.is_dir());
        assert_eq!(fs::read(final_path.join("a.txt")).unwrap(), b"hello");
    }

    #[test]
    fn commit_refuses_existing_target_without_overwrite() {
        let root = TempDir::new().unwrap();
        let final_path = root.path().join("target");
        fs::create_dir(&final_path).unwrap();
        let staging = AtomicDir::stage(&final_path).unwrap();
        let err = staging.commit(false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(final_path.is_dir(), "target must be untouched");
    }

    #[test]
    fn commit_overwrite_replaces_existing_target() {
        let root = TempDir::new().unwrap();
        let final_path = root.path().join("target");
        fs::create_dir(&final_path).unwrap();
        fs::write(final_path.join("old.txt"), b"old").unwrap();
        let staging = AtomicDir::stage(&final_path).unwrap();
        fs::write(staging.path().join("new.txt"), b"new").unwrap();
        staging.commit(true).unwrap();
        assert!(final_path.join("new.txt").exists());
        assert!(!final_path.join("old.txt").exists());
    }

    #[test]
    fn drop_without_commit_removes_staging() {
        let root = TempDir::new().unwrap();
        let final_path = root.path().join("target");
        let staging_path = {
            let staging = AtomicDir::stage(&final_path).unwrap();
            staging.path().to_path_buf()
        };
        assert!(!staging_path.exists());
    }

    #[test]
    fn sweep_orphans_removes_only_staging_prefix_dirs() {
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join(".omni-staging-abc123")).unwrap();
        fs::create_dir(root.path().join(".omni-staging-def456")).unwrap();
        fs::create_dir(root.path().join("keep-me")).unwrap();
        fs::write(root.path().join("keep-me/file"), b"x").unwrap();
        let n = sweep_orphans(root.path()).unwrap();
        assert_eq!(n, 2);
        assert!(!root.path().join(".omni-staging-abc123").exists());
        assert!(!root.path().join(".omni-staging-def456").exists());
        assert!(root.path().join("keep-me").is_dir());
    }
}
