//! Atomic file write — temp → fsync → rename → fsync parent.
//!
//! Mirror of `crates/identity/src/atomic.rs::atomic_write`, kept separate
//! because identity's helper is `pub(crate)` and exposing it would be a
//! public-API change to identity. Per writing-lessons §F: small, focused,
//! belongs in the crate that uses it.
//!
//! Semantics match the identity helper exactly:
//! * Write to a sibling tempfile named `<path>.tmp`.
//! * `fsync` the data file before rename so the renamed inode points at
//!   durable bytes (defends against power-loss between `write` and `fsync`).
//! * Rename atomically replaces the target on POSIX + on NTFS.
//! * On Unix, `fsync` the parent directory afterwards so the rename itself
//!   is durable across crash. Windows has no portable parent-fsync — the
//!   `let _ = parent;` keeps the variable used and matches identity's shape.
//! * On Unix the tempfile is created mode `0o600` so brief filesystem
//!   exposure of secret material (identity rotation, future sealed
//!   manifests) doesn't leak through default umask.
//!
//! Errors surface as `std::io::Error` rather than the identity crate's
//! `IdentityError::Io(String)` because host-side callers (e.g. share
//! identity-metadata persistence) already operate in `std::io::Result`.

use std::fs;
use std::io;
use std::io::Write;
use std::path::Path;

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    let tmp = {
        let mut s = path.as_os_str().to_owned();
        s.push(".tmp");
        std::path::PathBuf::from(s)
    };

    {
        #[cfg(unix)]
        let mut f = {
            use std::os::unix::fs::OpenOptionsExt;
            let mut o = fs::OpenOptions::new();
            o.create(true).write(true).truncate(true).mode(0o600);
            o.open(&tmp)?
        };
        #[cfg(not(unix))]
        let mut f = {
            fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)?
        };
        f.write_all(bytes)?;
        f.sync_all()?;
    }

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    #[cfg(unix)]
    {
        if let Ok(dir) = fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_file_exactly() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("x.bin");
        atomic_write(&p, b"hello").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"hello");
    }

    #[test]
    fn overwrites_existing() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("x.bin");
        fs::write(&p, b"old").unwrap();
        atomic_write(&p, b"new").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new");
    }

    #[test]
    fn leaves_no_temp_file_on_success() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("x.bin");
        atomic_write(&p, b"data").unwrap();
        let tmp = dir.path().join("x.bin.tmp");
        assert!(!tmp.exists(), "tmp should be renamed away");
    }

    #[test]
    fn errors_if_parent_missing() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("no").join("x.bin");
        let err = atomic_write(&p, b"data").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }
}
