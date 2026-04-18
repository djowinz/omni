//! Pure-Rust core of the hand-rolled JWS compact EdDSA path used by the WASM
//! build (see `src/wasm.rs`). Extracted so the native test suite can exercise
//! the exact same signing bytes the Worker WASM runtime produces, to enforce
//! native↔wasm byte-parity (plan #008 W1T2 post-gate).
//!
//! This module MUST NOT depend on `wasm-bindgen`, `js-sys`, or `JsValue` —
//! it is the algorithmic core only. `src/wasm.rs` wraps it with the
//! wasm-bindgen error conversions.
//!
//! Two signing paths live here:
//!
//! 1. `sign_http_jws_compact_core` — the HTTP-auth path. Matches
//!    `identity::http_jws::sign_http_jws` on the shipped `theme-sharing`
//!    branch (host #010 consumer). Header: `{"typ":"Omni-HTTP-JWS","alg":"EdDSA"}`.
//!    Claims: the fixed-field `HttpJwsClaims` shape (alg/crv/typ/kid/df/ts/...).
//!    No embedded JWK — `kid` carries the base64-encoded pubkey instead.
//!
//! 2. `sign_jws_compact_core` — the signed-bundle path. Kept for
//!    `packSignedBundle` which embeds the author pubkey as an OKP JWK in the
//!    header so the verifier can extract it from the envelope alone. Header:
//!    `{"typ":"JWT","alg":"EdDSA","jwk":{...}}`.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};

const B64URL: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

// Field order below is load-bearing: it must match the byte-for-byte output
// of `jsonwebtoken` v9 (the native oracle in `keypair::sign_jws`). Changing
// the order of these struct fields WILL shift the JWS envelope bytes and
// break host↔Worker auth. Do not reorder without re-running the parity gate
// at `tests/jws_native_wasm_parity.rs`.
//
// Native `jsonwebtoken::Header` serializes as `{typ, alg, cty, jku, jwk, kid,
// ...}` (declaration order with `skip_serializing_if = "Option::is_none"` on
// every optional). For our two paths this resolves to:
//   HTTP-auth path: {"typ":"Omni-HTTP-JWS","alg":"EdDSA"}
//   bundle   path : {"typ":"JWT","alg":"EdDSA","jwk":{...}}
// `jsonwebtoken::jwk::Jwk` flattens `AlgorithmParameters`, so an OctetKeyPair
// serializes as {kty, crv, x} (kty from flattened AlgorithmParameters first,
// then crv, then x in declaration order).

#[derive(Serialize, Deserialize)]
pub(crate) struct OkpJwk {
    pub kty: String,
    pub crv: String,
    pub x: String,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct JwsHeader {
    pub typ: String,
    pub alg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jwk: Option<OkpJwk>,
}

/// WASM-side mirror of `identity::http_jws::HttpJwsClaims` (shipped on
/// the `theme-sharing` branch). Field declaration order is load-bearing:
/// `serde_json` serializes struct fields in declaration order, and the native
/// oracle (`jsonwebtoken::encode(header, &HttpJwsClaims, ..)`) does the same.
/// Any divergence between this struct and the shipped one will break byte
/// parity and silently split host↔Worker auth.
///
/// `kid` / `df` are STANDARD base64 (`+/=`), not URL-safe — this matches
/// what the shipped `sign_http_jws` does. The JWS envelope itself still uses
/// URL-safe-no-pad base64; only these two claim fields use standard base64.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WasmHttpJwsClaims {
    pub alg: String,
    pub crv: String,
    pub typ: String,
    pub kid: String,
    pub df: String,
    pub ts: i64,
    pub method: String,
    pub path: String,
    pub query_sha256: String,
    pub body_sha256: String,
    pub sanitize_version: u32,
}

/// Sign an `HttpJwsClaims`-shaped payload as a compact EdDSA JWS using `seed`
/// as the Ed25519 private-key seed. Emits header
/// `{"typ":"Omni-HTTP-JWS","alg":"EdDSA"}` — matches the native oracle
/// `identity::http_jws::sign_http_jws` byte-for-byte.
pub(crate) fn sign_http_jws_compact_core(
    claims: &WasmHttpJwsClaims,
    seed: &[u8; 32],
) -> Result<String, String> {
    let sk = SigningKey::from_bytes(seed);
    let header = JwsHeader {
        typ: "Omni-HTTP-JWS".into(),
        alg: "EdDSA".into(),
        jwk: None,
    };
    let header_bytes = serde_json::to_vec(&header).map_err(|e| e.to_string())?;
    let payload_bytes = serde_json::to_vec(claims).map_err(|e| e.to_string())?;
    let header_b64 = B64URL.encode(&header_bytes);
    let payload_b64 = B64URL.encode(&payload_bytes);
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = sk.sign(signing_input.as_bytes());
    let sig_b64 = B64URL.encode(sig.to_bytes());
    Ok(format!("{signing_input}.{sig_b64}"))
}

/// Sign `claims` (any JSON value) as a compact EdDSA JWS using `seed` as the
/// Ed25519 private-key seed, with the pubkey embedded as an OKP/Ed25519 JWK
/// in the header. Used by the signed-bundle flow so the verifier can extract
/// the author key from the envelope alone. Header: `{"typ":"JWT","alg":"EdDSA","jwk":{...}}`.
pub(crate) fn sign_bundle_jws_compact_core(
    claims: &serde_json::Value,
    seed: &[u8; 32],
) -> Result<String, String> {
    let sk = SigningKey::from_bytes(seed);
    let pk = sk.verifying_key().to_bytes();
    let header = JwsHeader {
        typ: "JWT".into(),
        alg: "EdDSA".into(),
        jwk: Some(OkpJwk {
            kty: "OKP".into(),
            crv: "Ed25519".into(),
            x: B64URL.encode(pk),
        }),
    };
    let header_bytes = serde_json::to_vec(&header).map_err(|e| e.to_string())?;
    let payload_bytes = serde_json::to_vec(claims).map_err(|e| e.to_string())?;
    let header_b64 = B64URL.encode(&header_bytes);
    let payload_b64 = B64URL.encode(&payload_bytes);
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = sk.sign(signing_input.as_bytes());
    let sig_b64 = B64URL.encode(sig.to_bytes());
    Ok(format!("{signing_input}.{sig_b64}"))
}
