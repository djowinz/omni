//! Ed25519 Keypair with persistent local storage and encrypted backup.

use std::path::Path;

use ed25519_dalek::{Signer, SigningKey};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation};
use rand::rngs::OsRng;
use serde::{de::DeserializeOwned, Serialize};
use zeroize::Zeroizing;

use crate::error::IdentityError;
use crate::fingerprint::{Fingerprint, PublicKey};

/// Wrap a 32-byte Ed25519 seed in a minimal PKCS#8 v1 private-key DER envelope.
///
/// Layout (RFC 8410):
///   SEQUENCE {
///     INTEGER 0,                              -- version
///     AlgorithmIdentifier { OID 1.3.101.112 }, -- id-Ed25519
///     OCTET STRING (OCTET STRING seed)         -- nested
///   }
fn pkcs8_ed25519_private(seed: &[u8; 32]) -> Zeroizing<Vec<u8>> {
    // Fixed-length PKCS#8 prefix for Ed25519 private keys (48 bytes total).
    const PREFIX: &[u8] = &[
        0x30, 0x2e, // SEQUENCE, length 46
        0x02, 0x01, 0x00, // INTEGER version=0
        0x30, 0x05, // SEQUENCE (AlgorithmIdentifier), length 5
        0x06, 0x03, 0x2b, 0x65, 0x70, // OID 1.3.101.112 (Ed25519)
        0x04, 0x22, // OCTET STRING, length 34
        0x04, 0x20, // inner OCTET STRING, length 32
    ];
    let mut out = Zeroizing::new(Vec::with_capacity(PREFIX.len() + 32));
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(seed);
    out
}

/// Verify a JWS compact string signed with EdDSA against `pubkey`, returning
/// the decoded claims.
///
/// Returns `IdentityError::Jws` on header/alg mismatch, signature failure, or
/// decode error. The pubkey is the author's raw 32-byte Ed25519 key.
pub fn verify_jws<T: DeserializeOwned>(
    jws: &str,
    pubkey: &PublicKey,
) -> Result<TokenData<T>, IdentityError> {
    // jsonwebtoken's `from_ed_der` for the decoding path passes bytes directly
    // to ring's `UnparsedPublicKey::new(&ED25519, key).verify(...)`, which
    // requires exactly 32 raw bytes. The name `from_ed_der` is misleading for
    // the public-key case — raw bytes are what ring expects here.
    let key = DecodingKey::from_ed_der(&pubkey.0);
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.validate_exp = false; // callers opt in to exp via their own claims type
    validation.validate_nbf = false;
    validation.required_spec_claims.clear();
    jsonwebtoken::decode::<T>(jws, &key, &validation)
        .map_err(|e| IdentityError::Jws(format!("decode: {e}")))
}

/// Ed25519 signing key.
///
/// Invariant: do not derive `Debug`, `Clone`, or `Copy`. `SigningKey` from
/// ed25519-dalek implements `ZeroizeOnDrop` internally, which would be
/// subverted by `Clone` (yields a non-zeroizing copy) or `Debug` (could leak
/// key bytes through formatting).
pub struct Keypair {
    signing: SigningKey,
}

impl Keypair {
    // DPAPI-backed identity.key storage (per sub-spec 006 §3, Phase-2 stretch
    // goal) is deferred to a follow-up ticket. Current on-disk protection
    // relies on the Windows DACL set by `crate::acl::set_user_only`.

    pub fn generate() -> Self {
        let mut rng = OsRng;
        Self {
            signing: SigningKey::generate(&mut rng),
        }
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

    /// Single signing authority for host→Worker request payloads.
    ///
    /// Per retro-004 D-004-A, `omni-identity::Keypair` signs all system payloads
    /// (bundle canonical hashes AND request bodies). This is a thin alias over
    /// [`Keypair::sign`] kept separate so call sites document their intent; if
    /// the request envelope ever switches to JWS (open question for sub-spec
    /// #008), callers move to [`Keypair::sign_jws`] without touching this one.
    pub fn sign_request(&self, canonical_bytes: &[u8]) -> [u8; 64] {
        self.sign(canonical_bytes)
    }

    /// JWS compact-serialize `claims` with `EdDSA` and return the compact string.
    ///
    /// The caller constructs `header` via `jsonwebtoken::Header::new(Algorithm::EdDSA)`
    /// and MAY set `kid`, `typ`, `cty`. The `alg` field is forced to `EdDSA`
    /// defensively — any other value returns `IdentityError::Jws`.
    ///
    /// Per retro-005 D3, this wraps an off-the-shelf JWS crate rather than
    /// hand-rolling the envelope.
    pub fn sign_jws<T: Serialize>(
        &self,
        claims: &T,
        header: &Header,
    ) -> Result<String, IdentityError> {
        if header.alg != Algorithm::EdDSA {
            return Err(IdentityError::Jws(format!(
                "alg must be EdDSA, got {:?}",
                header.alg
            )));
        }
        let seed = self.seed();
        let der = pkcs8_ed25519_private(&seed);
        let key = EncodingKey::from_ed_der(&der);
        jsonwebtoken::encode(header, claims, &key)
            .map_err(|e| IdentityError::Jws(format!("encode: {e}")))
    }

    /// Expose the 32-byte seed. Callers MUST zeroize after use.
    pub(crate) fn seed(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.signing.to_bytes())
    }

    pub(crate) fn from_seed(seed: &[u8; 32]) -> Self {
        Self {
            signing: SigningKey::from_bytes(seed),
        }
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

    pub fn export_encrypted(&self, passphrase: &str) -> Result<Vec<u8>, IdentityError> {
        use argon2::{Algorithm, Argon2, Params, Version};
        use chacha20poly1305::{
            aead::{Aead, KeyInit, Payload},
            XChaCha20Poly1305, XNonce,
        };
        use rand::RngCore;
        use zeroize::Zeroizing;

        use crate::format::*;

        let seed = self.seed();
        let plaintext = encode_identity_key(&seed);

        let mut salt = [0u8; SALT_LEN];
        let mut nonce_bytes = [0u8; NONCE_LEN];
        let mut rng = rand::rngs::OsRng;
        rng.fill_bytes(&mut salt);
        rng.fill_bytes(&mut nonce_bytes);

        let params = Params::new(
            ARGON2_M_COST_KIB,
            ARGON2_T_COST,
            ARGON2_P_COST,
            Some(ARGON2_OUTPUT_LEN as usize),
        )
        .map_err(|e| IdentityError::Crypto(format!("argon2 params: {e}")))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key = Zeroizing::new([0u8; 32]);
        argon2
            .hash_password_into(passphrase.as_bytes(), &salt, &mut *key)
            .map_err(|e| IdentityError::Crypto(format!("argon2: {e}")))?;

        let params_blob = encode_params_blob();

        // AAD = magic (10) || version (1) || params_blob (32)
        let mut aad = Vec::with_capacity(10 + 1 + PARAMS_LEN);
        aad.extend_from_slice(BACKUP_MAGIC);
        aad.push(BACKUP_VERSION);
        aad.extend_from_slice(&params_blob);

        let cipher = XChaCha20Poly1305::new_from_slice(&*key)
            .map_err(|e| IdentityError::Crypto(format!("cipher key: {e}")))?;
        let nonce = XNonce::from_slice(&nonce_bytes);
        let ct_and_tag = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .map_err(|e| IdentityError::Crypto(format!("encrypt: {e}")))?;

        debug_assert_eq!(ct_and_tag.len(), PLAINTEXT_LEN + TAG_LEN);

        let mut out = Vec::with_capacity(BACKUP_LEN);
        out.extend_from_slice(BACKUP_MAGIC);
        out.push(BACKUP_VERSION);
        out.extend_from_slice(&salt);
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&params_blob);
        out.extend_from_slice(&ct_and_tag);
        debug_assert_eq!(out.len(), BACKUP_LEN);
        Ok(out)
    }

    pub fn import_encrypted(bytes: &[u8], passphrase: &str) -> Result<Self, IdentityError> {
        use argon2::{Algorithm, Argon2, Params, Version};
        use chacha20poly1305::{
            aead::{Aead, KeyInit, Payload},
            XChaCha20Poly1305, XNonce,
        };
        use zeroize::Zeroizing;

        use crate::format::*;

        if bytes.len() != BACKUP_LEN {
            return Err(IdentityError::Corrupt("bad length".into()));
        }
        if &bytes[OFFSET_MAGIC..OFFSET_MAGIC + 10] != BACKUP_MAGIC {
            return Err(IdentityError::BadMagic);
        }
        if bytes[OFFSET_VERSION] != BACKUP_VERSION {
            return Err(IdentityError::UnsupportedVersion(bytes[OFFSET_VERSION]));
        }
        let salt = &bytes[OFFSET_SALT..OFFSET_SALT + SALT_LEN];
        let nonce_bytes = &bytes[OFFSET_NONCE..OFFSET_NONCE + NONCE_LEN];
        let params_blob = &bytes[OFFSET_PARAMS..OFFSET_PARAMS + PARAMS_LEN];
        let params = decode_params_blob(params_blob)?;
        let ct_and_tag = &bytes[OFFSET_CIPHERTEXT..BACKUP_LEN];

        let a2_params = Params::new(
            params.m_cost_kib,
            params.t_cost,
            params.p_cost,
            Some(params.output_len as usize),
        )
        .map_err(|e| IdentityError::Crypto(format!("argon2 params: {e}")))?;
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, a2_params);
        let mut key = Zeroizing::new([0u8; 32]);
        argon2
            .hash_password_into(passphrase.as_bytes(), salt, &mut *key)
            .map_err(|e| IdentityError::Crypto(format!("argon2: {e}")))?;

        let mut aad = Vec::with_capacity(10 + 1 + PARAMS_LEN);
        aad.extend_from_slice(BACKUP_MAGIC);
        aad.push(BACKUP_VERSION);
        aad.extend_from_slice(params_blob);

        let cipher = XChaCha20Poly1305::new_from_slice(&*key)
            .map_err(|e| IdentityError::Crypto(format!("cipher key: {e}")))?;
        let nonce = XNonce::from_slice(nonce_bytes);
        let plaintext = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ct_and_tag,
                    aad: &aad,
                },
            )
            .map_err(|_| IdentityError::BadPassphrase)?;
        let plaintext = Zeroizing::new(plaintext);

        let seed = decode_identity_key(&plaintext)?;
        Ok(Self::from_seed(&seed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Verifier, VerifyingKey};
    use jsonwebtoken::{Algorithm, Header};
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct TestClaims {
        h: String,
        iat: u64,
    }

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

    #[test]
    fn export_import_round_trip() {
        let kp = Keypair::generate();
        let bak = kp.export_encrypted("correct horse battery staple").unwrap();
        assert_eq!(bak.len(), 173);
        let kp2 = Keypair::import_encrypted(&bak, "correct horse battery staple").unwrap();
        assert_eq!(kp.public_key(), kp2.public_key());
    }

    #[test]
    fn import_rejects_wrong_passphrase() {
        let kp = Keypair::generate();
        let bak = kp.export_encrypted("right").unwrap();
        assert!(matches!(
            Keypair::import_encrypted(&bak, "wrong"),
            Err(IdentityError::BadPassphrase)
        ));
    }

    #[test]
    fn import_rejects_bad_magic() {
        let kp = Keypair::generate();
        let mut bak = kp.export_encrypted("pw").unwrap();
        bak[0] = b'X';
        assert!(matches!(
            Keypair::import_encrypted(&bak, "pw"),
            Err(IdentityError::BadMagic)
        ));
    }

    #[test]
    fn import_rejects_bad_length() {
        assert!(matches!(
            Keypair::import_encrypted(&[0u8; 10], "pw"),
            Err(IdentityError::Corrupt(_))
        ));
    }

    #[test]
    fn import_rejects_bad_version() {
        let kp = Keypair::generate();
        let mut bak = kp.export_encrypted("pw").unwrap();
        bak[10] = 0x02;
        assert!(matches!(
            Keypair::import_encrypted(&bak, "pw"),
            Err(IdentityError::UnsupportedVersion(2))
        ));
    }

    #[test]
    fn import_rejects_tampered_ciphertext() {
        let kp = Keypair::generate();
        let mut bak = kp.export_encrypted("pw").unwrap();
        bak[100] ^= 0xFF;
        assert!(matches!(
            Keypair::import_encrypted(&bak, "pw"),
            Err(IdentityError::BadPassphrase)
        ));
    }

    #[test]
    fn seed_is_zeroizing_wrapper() {
        // Compile-time check: seed() must return Zeroizing<[u8; 32]>
        let kp = Keypair::generate();
        let s: zeroize::Zeroizing<[u8; 32]> = kp.seed();
        assert_eq!(s.len(), 32);
    }

    #[test]
    fn sign_request_is_alias_for_sign() {
        let kp = Keypair::generate();
        let msg = b"canonical-request-body";
        assert_eq!(kp.sign_request(msg), kp.sign(msg));
    }

    #[test]
    fn jws_round_trip_with_typed_claims() {
        let kp = Keypair::generate();
        let claims = TestClaims {
            h: "deadbeef".into(),
            iat: 1_700_000_000,
        };
        let header = Header::new(Algorithm::EdDSA);
        let jws = kp.sign_jws(&claims, &header).unwrap();
        // Compact JWS has exactly two dots (3 base64url segments).
        assert_eq!(jws.matches('.').count(), 2);
        let decoded = super::verify_jws::<TestClaims>(&jws, &kp.public_key()).unwrap();
        assert_eq!(decoded.claims, claims);
        assert_eq!(decoded.header.alg, Algorithm::EdDSA);
    }

    #[test]
    fn jws_rejects_non_eddsa_header() {
        let kp = Keypair::generate();
        let claims = TestClaims {
            h: "x".into(),
            iat: 0,
        };
        let mut header = Header::new(Algorithm::HS256);
        // NOTE: Header::new(HS256) is accepted by jsonwebtoken for HMAC; we
        // reject defensively in sign_jws.
        let err = kp.sign_jws(&claims, &header).unwrap_err();
        assert!(matches!(err, IdentityError::Jws(_)));
        // sanity: EdDSA path works
        header = Header::new(Algorithm::EdDSA);
        kp.sign_jws(&claims, &header).unwrap();
    }

    #[test]
    fn jws_rejects_wrong_pubkey() {
        let signer = Keypair::generate();
        let attacker_view = Keypair::generate(); // different keypair
        let claims = TestClaims {
            h: "x".into(),
            iat: 0,
        };
        let header = Header::new(Algorithm::EdDSA);
        let jws = signer.sign_jws(&claims, &header).unwrap();
        let err = super::verify_jws::<TestClaims>(&jws, &attacker_view.public_key()).unwrap_err();
        assert!(matches!(err, IdentityError::Jws(_)));
    }

    #[test]
    fn jws_rejects_tampered_payload() {
        let kp = Keypair::generate();
        let claims = TestClaims {
            h: "x".into(),
            iat: 0,
        };
        let header = Header::new(Algorithm::EdDSA);
        let jws = kp.sign_jws(&claims, &header).unwrap();
        // Tamper with the signature segment (third part). The Ed25519 signature
        // encodes as 86 base64url chars, guaranteed to have a wide variety of
        // bytes. Flip the first char: if it's alphanumeric use '_', otherwise
        // use 'A'. Either mutation produces an invalid signature.
        let mut parts: Vec<String> = jws.split('.').map(str::to_owned).collect();
        assert_eq!(parts.len(), 3);
        let mut sig = parts[2].clone().into_bytes();
        // Replace the first byte with a guaranteed-different valid base64url byte.
        sig[0] = if sig[0] == b'A' { b'B' } else { b'A' };
        parts[2] = String::from_utf8(sig).unwrap();
        let tampered = parts.join(".");
        let err = super::verify_jws::<TestClaims>(&tampered, &kp.public_key()).unwrap_err();
        assert!(matches!(err, IdentityError::Jws(_)));
    }
}
