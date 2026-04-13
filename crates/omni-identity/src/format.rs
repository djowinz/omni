//! Byte-level encode/decode for `identity.key` per identity-file-format.md §1.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use crate::error::IdentityError;

type HmacSha256 = Hmac<Sha256>;

pub(crate) const IDENTITY_KEY_LEN: usize = 74;
pub(crate) const IDENTITY_KEY_MAGIC: &[u8; 9] = b"OMNI-IDv1";
pub(crate) const IDENTITY_KEY_VERSION: u8 = 0x01;
pub(crate) const IDENTITY_KEY_HMAC_KEY: &[u8] = b"omni-id-local";

pub(crate) fn encode_identity_key(seed: &[u8; 32]) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(Vec::with_capacity(IDENTITY_KEY_LEN));
    out.extend_from_slice(IDENTITY_KEY_MAGIC);
    out.push(IDENTITY_KEY_VERSION);
    out.extend_from_slice(seed);

    let mut mac = <HmacSha256 as Mac>::new_from_slice(IDENTITY_KEY_HMAC_KEY).expect("hmac key");
    mac.update(&out[..42]);
    let tag = mac.finalize().into_bytes();
    out.extend_from_slice(&tag);
    debug_assert_eq!(out.len(), IDENTITY_KEY_LEN);
    out
}

pub(crate) fn decode_identity_key(bytes: &[u8]) -> Result<Zeroizing<[u8; 32]>, IdentityError> {
    if bytes.len() != IDENTITY_KEY_LEN {
        return Err(IdentityError::Corrupt("bad length".into()));
    }
    if &bytes[0..9] != IDENTITY_KEY_MAGIC {
        return Err(IdentityError::BadMagic);
    }
    if bytes[9] != IDENTITY_KEY_VERSION {
        return Err(IdentityError::UnsupportedVersion(bytes[9]));
    }

    let mut mac = <HmacSha256 as Mac>::new_from_slice(IDENTITY_KEY_HMAC_KEY).expect("hmac key");
    mac.update(&bytes[0..42]);
    let expected = mac.finalize().into_bytes();
    if expected.as_slice().ct_eq(&bytes[42..74]).unwrap_u8() != 1 {
        return Err(IdentityError::BadChecksum);
    }

    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&bytes[10..42]);
    Ok(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let seed = [7u8; 32];
        let enc = encode_identity_key(&seed);
        assert_eq!(enc.len(), 74);
        let dec = decode_identity_key(&enc).unwrap();
        assert_eq!(*dec, seed);
    }

    #[test]
    fn rejects_bad_length() {
        let err = decode_identity_key(&[0u8; 73]).unwrap_err();
        assert!(matches!(err, IdentityError::Corrupt(_)));
    }

    #[test]
    fn rejects_bad_magic() {
        let seed = [0u8; 32];
        let mut enc = encode_identity_key(&seed).to_vec();
        enc[0] = b'X';
        let err = decode_identity_key(&enc).unwrap_err();
        assert!(matches!(err, IdentityError::BadMagic));
    }

    #[test]
    fn rejects_bad_version() {
        let seed = [0u8; 32];
        let mut enc = encode_identity_key(&seed).to_vec();
        enc[9] = 0x02;
        let err = decode_identity_key(&enc).unwrap_err();
        assert!(matches!(err, IdentityError::UnsupportedVersion(2)));
    }

    #[test]
    fn rejects_tampered_seed() {
        let seed = [1u8; 32];
        let mut enc = encode_identity_key(&seed).to_vec();
        enc[20] ^= 0xFF;
        let err = decode_identity_key(&enc).unwrap_err();
        assert!(matches!(err, IdentityError::BadChecksum));
    }

    #[test]
    fn magic_matches_spec() {
        assert_eq!(IDENTITY_KEY_MAGIC, b"OMNI-IDv1");
        assert_eq!(IDENTITY_KEY_LEN, 74);
    }
}
