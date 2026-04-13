//! Atomic file write: temp → fsync → rename → fsync parent.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::IdentityError;

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), IdentityError> {
    let parent = path
        .parent()
        .ok_or_else(|| IdentityError::Io("path has no parent".to_string()))?;
    let tmp = {
        let mut s = path.as_os_str().to_owned();
        s.push(".tmp");
        std::path::PathBuf::from(s)
    };

    {
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }

    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(IdentityError::Io(e.to_string()));
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
        assert!(matches!(err, IdentityError::Io(_)));
    }
}
