//! Omni local identity: Ed25519 keypair, fingerprint, and encrypted backup format.
//! Bodies are `todo!()` except trivially-implementable helpers.

use std::fmt;
use std::path::Path;

use ed25519_dalek::SigningKey;

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
}

pub struct Keypair(SigningKey);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKey(pub [u8; 32]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint(pub [u8; 6]);

impl Keypair {
    pub fn generate() -> Self {
        todo!("implemented in sub-spec 006")
    }

    pub fn load_or_create(_path: &Path) -> Result<Self, IdentityError> {
        todo!("implemented in sub-spec 006")
    }

    pub fn public_key(&self) -> PublicKey {
        todo!("implemented in sub-spec 006")
    }

    pub fn fingerprint(&self) -> Fingerprint {
        todo!("implemented in sub-spec 006")
    }

    pub fn sign(&self, _msg: &[u8]) -> [u8; 64] {
        todo!("implemented in sub-spec 006")
    }

    pub fn export_encrypted(&self, _passphrase: &str) -> Result<Vec<u8>, IdentityError> {
        todo!("implemented in sub-spec 006")
    }

    pub fn import_encrypted(
        _bytes: &[u8],
        _passphrase: &str,
    ) -> Result<Self, IdentityError> {
        todo!("implemented in sub-spec 006")
    }
}

impl Fingerprint {
    pub fn to_words(&self) -> [&'static str; 3] {
        todo!("implemented in sub-spec 006 (BIP-39 lookup)")
    }

    pub fn to_emoji(&self) -> [&'static str; 6] {
        todo!("implemented in sub-spec 006")
    }

    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(12);
        for b in &self.0 {
            s.push_str(&format!("{:02x}", b));
        }
        s
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{:02x}", b)?;
        }
        Ok(())
    }
}
