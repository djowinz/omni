//! Ed25519 Keypair with persistent local storage and encrypted backup.

use std::path::Path;

use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::error::IdentityError;
use crate::fingerprint::{Fingerprint, PublicKey};

pub struct Keypair {
    signing: SigningKey,
}

impl Keypair {
    pub fn generate() -> Self {
        let mut rng = OsRng;
        Self { signing: SigningKey::generate(&mut rng) }
    }

    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.signing.verifying_key().to_bytes())
    }

    pub fn fingerprint(&self) -> Fingerprint {
        self.public_key().fingerprint()
    }

    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing.sign(msg).to_bytes()
    }

    /// Expose the 32-byte seed. Callers MUST zeroize after use.
    pub(crate) fn seed(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.signing.to_bytes())
    }

    pub(crate) fn from_seed(seed: &[u8; 32]) -> Self {
        Self { signing: SigningKey::from_bytes(seed) }
    }

    pub fn load_or_create(path: &Path) -> Result<Self, IdentityError> {
        if path.exists() {
            let bytes = std::fs::read(path)?;
            let seed = crate::format::decode_identity_key(&bytes)?;
            #[cfg(windows)]
            {
                crate::acl::verify_user_only(path)?;
            }
            Ok(Self::from_seed(&seed))
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let kp = Self::generate();
            let seed = kp.seed();
            let enc = crate::format::encode_identity_key(&seed);
            crate::atomic::atomic_write(path, &enc)?;
            #[cfg(windows)]
            {
                crate::acl::set_user_only(path)?;
            }
            Ok(kp)
        }
    }

    pub fn export_encrypted(&self, _passphrase: &str) -> Result<Vec<u8>, IdentityError> {
        todo!("Task 10")
    }

    pub fn import_encrypted(_bytes: &[u8], _passphrase: &str) -> Result<Self, IdentityError> {
        todo!("Task 10")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Verifier, VerifyingKey};

    #[test]
    fn generate_produces_distinct_keys() {
        let a = Keypair::generate();
        let b = Keypair::generate();
        assert_ne!(a.public_key(), b.public_key());
    }

    #[test]
    fn sign_verifies_with_public_key() {
        let kp = Keypair::generate();
        let msg = b"hello omni";
        let sig = kp.sign(msg);
        let vk = VerifyingKey::from_bytes(&kp.public_key().0).unwrap();
        let sig = ed25519_dalek::Signature::from_bytes(&sig);
        assert!(vk.verify(msg, &sig).is_ok());
    }

    #[test]
    fn fingerprint_matches_pubkey_fingerprint() {
        let kp = Keypair::generate();
        assert_eq!(kp.fingerprint(), kp.public_key().fingerprint());
    }

    #[test]
    fn from_seed_round_trips() {
        let kp = Keypair::generate();
        let seed = kp.seed();
        let kp2 = Keypair::from_seed(&seed);
        assert_eq!(kp.public_key(), kp2.public_key());
    }

    use tempfile::tempdir;

    #[test]
    fn load_or_create_generates_on_first_call() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("id").join("identity.key");
        let kp = Keypair::load_or_create(&p).unwrap();
        assert!(p.exists());
        assert_eq!(std::fs::metadata(&p).unwrap().len(), 74);
        // Second call returns same key
        let kp2 = Keypair::load_or_create(&p).unwrap();
        assert_eq!(kp.public_key(), kp2.public_key());
    }

    #[test]
    fn load_rejects_corrupt_file() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("identity.key");
        std::fs::write(&p, b"garbage").unwrap();
        let result = Keypair::load_or_create(&p);
        assert!(matches!(result, Err(IdentityError::Corrupt(_))));
    }
}
