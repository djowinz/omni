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

// --- .omniid backup format (identity-file-format.md §2) ---

pub(crate) const BACKUP_LEN: usize = 173;
pub(crate) const BACKUP_MAGIC: &[u8; 10] = b"OMNI-IDBAK";
pub(crate) const BACKUP_VERSION: u8 = 0x01;

pub(crate) const ARGON2_M_COST_KIB: u32 = 65_536; // 64 MiB
pub(crate) const ARGON2_T_COST: u32 = 3;
pub(crate) const ARGON2_P_COST: u32 = 4;
pub(crate) const ARGON2_OUTPUT_LEN: u32 = 32;

pub(crate) const OFFSET_MAGIC: usize = 0;
pub(crate) const OFFSET_VERSION: usize = 10;
pub(crate) const OFFSET_SALT: usize = 11;
pub(crate) const OFFSET_NONCE: usize = 27;
pub(crate) const OFFSET_PARAMS: usize = 51;
pub(crate) const OFFSET_CIPHERTEXT: usize = 83;
pub(crate) const SALT_LEN: usize = 16;
pub(crate) const NONCE_LEN: usize = 24;
pub(crate) const PARAMS_LEN: usize = 32;
pub(crate) const PLAINTEXT_LEN: usize = IDENTITY_KEY_LEN; // 74
pub(crate) const TAG_LEN: usize = 16;

pub(crate) fn encode_params_blob() -> [u8; PARAMS_LEN] {
    let mut b = [0u8; PARAMS_LEN];
    b[0..4].copy_from_slice(&ARGON2_M_COST_KIB.to_le_bytes());
    b[4..8].copy_from_slice(&ARGON2_T_COST.to_le_bytes());
    b[8..12].copy_from_slice(&ARGON2_P_COST.to_le_bytes());
    b[12..16].copy_from_slice(&ARGON2_OUTPUT_LEN.to_le_bytes());
    b
}

#[derive(Debug)]
pub(crate) struct BackupParams {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
    pub output_len: u32,
}

pub(crate) fn decode_params_blob(b: &[u8]) -> Result<BackupParams, IdentityError> {
    if b.len() != PARAMS_LEN {
        return Err(IdentityError::Corrupt("params blob len".into()));
    }
    let m_cost_kib = u32::from_le_bytes(b[0..4].try_into().unwrap());
    let t_cost = u32::from_le_bytes(b[4..8].try_into().unwrap());
    let p_cost = u32::from_le_bytes(b[8..12].try_into().unwrap());
    let output_len = u32::from_le_bytes(b[12..16].try_into().unwrap());
    if output_len != ARGON2_OUTPUT_LEN {
        return Err(IdentityError::Corrupt("output_len != 32".into()));
    }
    Ok(BackupParams {
        m_cost_kib,
        t_cost,
        p_cost,
        output_len,
    })
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

#[cfg(test)]
mod backup_tests {
    use super::*;

    #[test]
    fn params_blob_round_trip() {
        let b = encode_params_blob();
        let p = decode_params_blob(&b).unwrap();
        assert_eq!(p.m_cost_kib, 65_536);
        assert_eq!(p.t_cost, 3);
        assert_eq!(p.p_cost, 4);
        assert_eq!(p.output_len, 32);
    }

    #[test]
    fn params_blob_rejects_bad_output_len() {
        let mut b = encode_params_blob();
        b[12] = 16; // output_len = 16 instead of 32
        let err = decode_params_blob(&b).unwrap_err();
        assert!(matches!(err, IdentityError::Corrupt(_)));
    }

    #[test]
    fn backup_constants_match_spec() {
        assert_eq!(BACKUP_LEN, 173);
        assert_eq!(BACKUP_MAGIC, b"OMNI-IDBAK");
        assert_eq!(OFFSET_CIPHERTEXT + PLAINTEXT_LEN + TAG_LEN, BACKUP_LEN);
    }
}
