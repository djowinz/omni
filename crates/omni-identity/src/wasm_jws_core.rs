//! Pure-Rust core of the hand-rolled JWS compact EdDSA path used by the WASM
//! build (see `src/wasm.rs`). Extracted so the native test suite can exercise
//! the exact same signing bytes the Worker WASM runtime produces, to enforce
//! native↔wasm byte-parity (plan #008 W1T2 post-gate).
//!
//! This module MUST NOT depend on `wasm-bindgen`, `js-sys`, or `JsValue` —
//! it is the algorithmic core only. `src/wasm.rs` wraps it with the
//! wasm-bindgen error conversions.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

// Field order below is load-bearing: it must match the byte-for-byte output
// of `jsonwebtoken` v9 (the native oracle in `keypair::sign_jws`). Changing
// the order of these struct fields WILL shift the JWS envelope bytes and
// break host↔Worker auth. Do not reorder without re-running the parity gate
// at `tests/jws_native_wasm_parity.rs`.
//
// Native `jsonwebtoken::Header` serializes as `{typ, alg, ..., jwk, ...}`.
// Native `jsonwebtoken::jwk::Jwk` with OctetKeyPair serializes its OKP
// parameters as `{kty, crv, x}` (kty comes first because it's flattened
// from `AlgorithmParameters`, then crv, then x in field order).

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct OkpJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct JwsHeader {
    pub typ: String,
    pub alg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwk: Option<OkpJwk>,
}

/// Sign `claims` (any JSON value) as a compact EdDSA JWS using `seed` as the
/// Ed25519 private-key seed. If `embed_jwk` is true, the header includes the
/// public key as an inline OKP/Ed25519 JWK (used by signed-bundle flows so the
/// verifier can extract the author key from the envelope alone).
///
/// This is the exact byte-level signing path used by the WASM build.
pub(crate) fn sign_jws_compact_core(
    claims: &serde_json::Value,
    seed: &[u8; 32],
    embed_jwk: bool,
) -> Result<String, String> {
    let sk = SigningKey::from_bytes(seed);
    let pk = sk.verifying_key().to_bytes();
    let header = JwsHeader {
        alg: "EdDSA".into(),
        typ: "JWT".into(),
        jwk: if embed_jwk {
            Some(OkpJwk {
                crv: "Ed25519".into(),
                kty: "OKP".into(),
                x: B64.encode(pk),
            })
        } else {
            None
        },
    };
    let header_bytes = serde_json::to_vec(&header).map_err(|e| e.to_string())?;
    let payload_bytes = serde_json::to_vec(claims).map_err(|e| e.to_string())?;
    let header_b64 = B64.encode(&header_bytes);
    let payload_b64 = B64.encode(&payload_bytes);
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = sk.sign(signing_input.as_bytes());
    let sig_b64 = B64.encode(sig.to_bytes());
    Ok(format!("{signing_input}.{sig_b64}"))
}
