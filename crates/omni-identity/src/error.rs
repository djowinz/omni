//! Error taxonomy for the omni-identity crate.

use std::io;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("io: {0}")]
    Io(String),
    #[error("file corrupt: {0}")]
    Corrupt(String),
    #[error("bad passphrase")]
    BadPassphrase,
    #[error("bad checksum")]
    BadChecksum,
    #[error("unsupported version: {0}")]
    UnsupportedVersion(u8),
    #[error("bad magic")]
    BadMagic,
    #[error("crypto: {0}")]
    Crypto(String),
    #[error("permission: {0}")]
    Permission(String),
    #[error("tofu registry: {0}")]
    Tofu(String),
}

impl From<io::Error> for IdentityError {
    fn from(e: io::Error) -> Self {
        IdentityError::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_round_trip() {
        let e = IdentityError::BadMagic;
        assert_eq!(e.to_string(), "bad magic");
        let e = IdentityError::UnsupportedVersion(2);
        assert_eq!(e.to_string(), "unsupported version: 2");
    }

    #[test]
    fn from_io_preserves_message() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "missing");
        let e: IdentityError = io_err.into();
        match e {
            IdentityError::Io(s) => assert!(s.contains("missing")),
            _ => panic!("wrong variant"),
        }
    }
}
