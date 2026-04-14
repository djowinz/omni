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
    Bundle(omni_bundle::BundleError),
}

impl From<io::Error> for IdentityError {
    fn from(e: io::Error) -> Self {
        IdentityError::Io(e.to_string())
    }
}

/// Map `omni_bundle::BundleError` categories into identity-level semantics.
///
/// Hand-written per retro-005 D9: do NOT use `#[from]`. Each category is
/// deliberately translated so that `IdentityError`'s public surface stays
/// stable even if `BundleError` categories evolve. Today every variant maps
/// pass-through; in follow-up work (signed-bundle surface), `Manifest` or
/// `HashMismatch` may translate to more specific identity semantics.
impl From<omni_bundle::BundleError> for IdentityError {
    fn from(e: omni_bundle::BundleError) -> Self {
        use omni_bundle::BundleError as B;
        match e {
            B::Zip(_)
            | B::Manifest(_)
            | B::MissingFile(_)
            | B::HashMismatch { .. }
            | B::SizeExceeded { .. }
            | B::TooManyEntries(_)
            | B::UnsafePath(_)
            | B::Io(_) => IdentityError::Bundle(e),
        }
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
        let be = omni_bundle::BundleError::UnsafePath("../evil".into());
        let ie: IdentityError = be.into();
        match ie {
            IdentityError::Bundle(omni_bundle::BundleError::UnsafePath(p)) => {
                assert_eq!(p, "../evil");
            }
            other => panic!("expected Bundle(UnsafePath), got {other}"),
        }
    }
}
