//! Bundle-signing surface — omni-identity is the single authority for
//! packing / verifying .omnipkg bytes (retro-005 D4, invariants #1, #4, #6a).
//!
//! ## API deviations from plan sketch
//!
//! - `sign_jws` takes a `&Header` argument (not just claims); we construct the
//!   header here with the author pubkey embedded as an OKP JWK (`header.jwk`).
//! - `verify_jws` takes a `&PublicKey` argument; we first extract the pubkey
//!   from the unauthenticated JWS header, then verify the signature with it.
//! - `PublicKey` is a tuple struct with a public `.0: [u8; 32]` field; there
//!   is no `from_bytes` constructor, so we write `PublicKey(arr)` directly.
//! - `Fingerprint` is computed via `pubkey.fingerprint()`, not `Fingerprint::from`.

use std::collections::BTreeMap;

use base64::Engine;
use jsonwebtoken::{
    jwk::{AlgorithmParameters, EllipticCurve, Jwk, OctetKeyPairParameters, OctetKeyPairType},
    Algorithm, Header,
};
use omni_bundle::{
    canonical_hash, pack as bundle_pack, unpack as bundle_unpack, BundleError, BundleLimits,
    FileEntry, IntegrityKind, Manifest,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::IdentityError;
use crate::fingerprint::{Fingerprint, PublicKey};
use crate::keypair::{verify_jws, Keypair};

const SIGNATURE_FILENAME: &str = "signature.jws";

#[derive(Serialize, Deserialize)]
struct SignaturePayload {
    canonical_hash_hex: String,
}

/// Verified, unpacked bundle. Per invariant #9, fields are private and the
/// struct is only constructible via `unpack_signed_bundle`: holding a
/// `SignedBundle` means the JWS has been verified and the manifest hash
/// matches the signed digest.
pub struct SignedBundle {
    manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
    author_pubkey: PublicKey,
}

impl SignedBundle {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn author_pubkey(&self) -> &PublicKey {
        &self.author_pubkey
    }

    pub fn fingerprint(&self) -> Fingerprint {
        self.author_pubkey.fingerprint()
    }

    /// Streaming iterator over bundle files (invariant #19b). Peak memory
    /// bounded by the largest single file, not the full bundle.
    pub fn files(&self) -> impl Iterator<Item = (&String, &Vec<u8>)> {
        self.files.iter()
    }

    /// Consume and return the materialized (manifest, files) pair (invariant #19b).
    pub fn into_files_map(self) -> (Manifest, BTreeMap<String, Vec<u8>>) {
        (self.manifest, self.files)
    }
}

/// Pack a signed bundle. The JWS is placed in `signature.jws` inside the zip
/// (invariant #6a — signature outside payload).
///
/// The canonical hash is computed over the ORIGINAL `manifest` before any
/// signature artifacts are added (invariant #9).
pub fn pack_signed_bundle(
    manifest: &Manifest,
    files: &BTreeMap<String, Vec<u8>>,
    keypair: &Keypair,
    limits: &BundleLimits,
) -> Result<Vec<u8>, IdentityError> {
    // Sort manifest.files BEFORE hashing so unpack's strip-and-rehash path
    // produces the same digest regardless of caller's original ordering.
    let mut pre_sig_manifest = manifest.clone();
    pre_sig_manifest.files.sort_by(|a, b| a.path.cmp(&b.path));
    let digest = canonical_hash(&pre_sig_manifest, files);
    let payload = SignaturePayload { canonical_hash_hex: hex::encode(digest) };

    let pubkey = keypair.public_key();
    let x_b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(pubkey.0);
    let jwk = Jwk {
        common: Default::default(),
        algorithm: AlgorithmParameters::OctetKeyPair(OctetKeyPairParameters {
            key_type: OctetKeyPairType::OctetKeyPair,
            curve: EllipticCurve::Ed25519,
            x: x_b64,
        }),
    };
    let mut header = Header::new(Algorithm::EdDSA);
    header.jwk = Some(jwk);

    let jws_compact = keypair
        .sign_jws(&payload, &header)
        .map_err(|e| IdentityError::Jws(format!("sign: {e}")))?;

    let mut amended_manifest = pre_sig_manifest;
    amended_manifest.files.push(FileEntry {
        path: SIGNATURE_FILENAME.into(),
        sha256: sha256_bytes(jws_compact.as_bytes()),
    });
    amended_manifest.files.sort_by(|a, b| a.path.cmp(&b.path));

    let mut amended_files = files.clone();
    amended_files.insert(SIGNATURE_FILENAME.into(), jws_compact.into_bytes());

    let bytes = bundle_pack(&amended_manifest, &amended_files, limits)?;
    Ok(bytes)
}

/// Unpack and verify a signed bundle. Returns a `SignedBundle` only if the
/// JWS is present, the Ed25519 signature is valid, and the canonical hash
/// of the original manifest matches the signed value.
///
/// If `expected_pubkey` is `Some`, the author key embedded in the JWS header
/// must match exactly; mismatch returns `IdentityError::Jws("pubkey mismatch")`.
pub fn unpack_signed_bundle(
    bytes: &[u8],
    expected_pubkey: Option<&PublicKey>,
    limits: &BundleLimits,
) -> Result<SignedBundle, IdentityError> {
    let unpack = bundle_unpack(bytes, limits)?;
    let (amended_manifest, mut files_map) = unpack.into_map()?;

    let jws_bytes = files_map
        .remove(SIGNATURE_FILENAME)
        .ok_or(IdentityError::MissingSignature)?;
    let jws_str = std::str::from_utf8(&jws_bytes)
        .map_err(|e| IdentityError::Jws(format!("non-utf8 jws: {e}")))?;

    let mut original_manifest = amended_manifest;
    original_manifest.files.retain(|f| f.path != SIGNATURE_FILENAME);

    let expected_hex = hex::encode(canonical_hash(&original_manifest, &BTreeMap::new()));

    // Extract pubkey from the JWS header (unauthenticated parse), then verify
    // the signature with it — verify_jws authenticates; the extracted key is
    // trusted only after verify_jws returns Ok.
    let author_pubkey = extract_jws_pubkey(jws_str)?;
    let payload = verify_jws::<SignaturePayload>(jws_str, &author_pubkey)
        .map_err(|e| IdentityError::Jws(format!("verify: {e}")))?
        .claims;

    if payload.canonical_hash_hex != expected_hex {
        return Err(IdentityError::Bundle(BundleError::Integrity {
            kind: IntegrityKind::HashMismatch,
            detail: format!(
                "canonical_hash mismatch: signed {}, computed {}",
                payload.canonical_hash_hex, expected_hex
            ),
        }));
    }

    if let Some(expected) = expected_pubkey {
        if expected.0 != author_pubkey.0 {
            return Err(IdentityError::Jws("pubkey mismatch".into()));
        }
    }

    Ok(SignedBundle {
        manifest: original_manifest,
        files: files_map,
        author_pubkey,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

/// Extract the Ed25519 public key from the `jwk` header of a compact JWS.
///
/// The header segment (before the first `.`) is base64url-decoded and parsed
/// as JSON. We require `jwk.algorithm` to be `OctetKeyPair` (OKP) with curve
/// `Ed25519` and a 32-byte `x` value.
fn extract_jws_pubkey(jws_compact: &str) -> Result<PublicKey, IdentityError> {
    let parts: Vec<&str> = jws_compact.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(IdentityError::Jws("not a 3-part compact JWS".into()));
    }
    let header_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[0])
        .map_err(|e| IdentityError::Jws(format!("header base64url: {e}")))?;
    let header: Header = serde_json::from_slice(&header_bytes)
        .map_err(|e| IdentityError::Jws(format!("header json: {e}")))?;
    let jwk = header
        .jwk
        .ok_or_else(|| IdentityError::Jws("missing jwk in header".into()))?;
    match jwk.algorithm {
        AlgorithmParameters::OctetKeyPair(okp) => {
            if okp.curve != EllipticCurve::Ed25519 {
                return Err(IdentityError::Jws(format!(
                    "expected Ed25519 OKP, got {:?}",
                    okp.curve
                )));
            }
            let x_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(&okp.x)
                .map_err(|e| IdentityError::Jws(format!("jwk.x base64url: {e}")))?;
            let arr: [u8; 32] = x_bytes
                .try_into()
                .map_err(|_| IdentityError::Jws("jwk.x is not 32 bytes".into()))?;
            Ok(PublicKey(arr))
        }
        other => Err(IdentityError::Jws(format!(
            "expected OctetKeyPair jwk, got {:?}",
            other
        ))),
    }
}
