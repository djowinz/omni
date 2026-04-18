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
    #[error("jws: {0}")]
    Jws(String),
    #[error("missing signature")]
    MissingSignature,
    #[error("bundle: {0}")]
    Bundle(bundle::BundleError),
}

impl From<io::Error> for IdentityError {
    fn from(e: io::Error) -> Self {
        IdentityError::Io(e.to_string())
    }
}

/// Map `bundle::BundleError` categories (retro-005 D9 shape) into
/// identity-level semantics. Hand-written per retro D9: no `#[from]`. The
/// public surface of `IdentityError` stays stable even if `BundleError`
/// sub-kinds evolve.
impl From<bundle::BundleError> for IdentityError {
    fn from(e: bundle::BundleError) -> Self {
        IdentityError::Bundle(e)
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

    #[test]
    fn jws_variant_displays() {
        let e = IdentityError::Jws("bad header".into());
        assert_eq!(e.to_string(), "jws: bad header");
    }

    #[test]
    fn missing_signature_variant_displays() {
        let e = IdentityError::MissingSignature;
        assert_eq!(e.to_string(), "missing signature");
    }

    #[test]
    fn bundle_error_from_preserves_category() {
        let be = bundle::BundleError::Unsafe {
            kind: bundle::UnsafeKind::Path,
            detail: "../evil".into(),
        };
        let ie: IdentityError = be.into();
        match ie {
            IdentityError::Bundle(bundle::BundleError::Unsafe { kind, detail }) => {
                assert_eq!(kind, bundle::UnsafeKind::Path);
                assert_eq!(detail, "../evil");
            }
            other => panic!("expected Bundle(Unsafe), got {other}"),
        }
    }
}
