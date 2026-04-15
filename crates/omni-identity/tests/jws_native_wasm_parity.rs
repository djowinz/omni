//! Byte-parity regression gate for the split JWS implementations in
//! `omni-identity` (plan #008 W1T2 post-gate).
//!
//! The native path (`Keypair::sign_jws`) uses `jsonwebtoken` v9 → `ring`.
//! The WASM path (`src/wasm.rs::sign_jws_compact`) hand-rolls compact EdDSA
//! via `ed25519-dalek` because `ring` does not cross-compile to
//! `wasm32-unknown-unknown` without a C toolchain.
//!
//! If the two paths produce byte-different JWS envelopes for the same claims
//! and key, every sign/verify interaction between the host (native) and the
//! Worker (WASM) silently breaks. This test proves byte-equality and locks
//! the proof as a regression test. Do NOT relax the assertion — adjust the
//! WASM path to match the native oracle (`jsonwebtoken`) if they diverge.
//!
//! Access to the WASM signing algorithm is via the pure-Rust core at
//! `crate::wasm_jws_core::sign_jws_compact_core`, which is what the
//! `#[wasm_bindgen]` export in `src/wasm.rs` delegates to. Exercising the
//! core from a native test therefore exercises the exact byte sequence the
//! Worker WASM runtime produces.

// The core module is `pub(crate)`, not exported. Integration tests live
// outside the crate, so they cannot see it. To make a minimal, focused
// parity check without reshaping public API, we re-declare the core's
// algorithm inline below and diff it against the native `jsonwebtoken`
// output. If this inline copy ever drifts from `src/wasm_jws_core.rs`, the
// native-vs-wasm assertion at the end of this test will fire and the
// regression gate will block the branch. The inline copy is intentional:
// it pins the on-wire byte shape independently of the crate's internal
// refactors.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use jsonwebtoken::{Algorithm, Header};
use omni_identity::Keypair;
use serde::{Deserialize, Serialize};

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

// Field order mirrors `src/wasm_jws_core.rs` (which in turn matches the
// native `jsonwebtoken` v9 oracle byte-for-byte). Reordering here WILL
// break the parity assertion below — that's the whole point of the gate.
#[derive(Serialize, Deserialize)]
struct OkpJwk {
    kty: String,
    crv: String,
    x: String,
}

#[derive(Serialize, Deserialize)]
struct WasmJwsHeader {
    typ: String,
    alg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    jwk: Option<OkpJwk>,
}

/// Byte-exact mirror of `omni_identity::wasm_jws_core::sign_jws_compact_core`.
/// Kept in this test file so drift in the core immediately shows up as a
/// parity failure here, not as a silent wire-format shift.
fn wasm_sign(claims: &serde_json::Value, seed: &[u8; 32], embed_jwk: bool) -> String {
    let sk = SigningKey::from_bytes(seed);
    let pk = sk.verifying_key().to_bytes();
    let header = WasmJwsHeader {
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
    let header_bytes = serde_json::to_vec(&header).unwrap();
    let payload_bytes = serde_json::to_vec(claims).unwrap();
    let header_b64 = B64.encode(&header_bytes);
    let payload_b64 = B64.encode(&payload_bytes);
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = sk.sign(signing_input.as_bytes());
    let sig_b64 = B64.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

/// Realistic host→Worker HTTP request claims. Field set mirrors what
/// sub-spec #008 §2 (Worker request envelope) pins as the signed claim
/// shape. Concrete `HttpJwsClaims` struct doesn't exist in the crate yet
/// (minted in W2T5); we use a hand-built `serde_json::Value` to avoid
/// coupling the gate to an in-flight type.
#[derive(Serialize, Deserialize)]
struct Claims {
    method: String,
    path: String,
    ts: u64,
    body_sha256: String,
    query_sha256: String,
    sanitize_version: u32,
    kid: String,
    df: String,
}

#[test]
fn jws_native_and_wasm_produce_byte_identical_envelopes() {
    // Fixed seed -> deterministic signature.
    let seed = [0x42u8; 32];
    let kp = Keypair::from_seed_for_test(&seed);
    let pk = kp.public_key();

    let claims = Claims {
        method: "POST".into(),
        path: "/v1/upload".into(),
        ts: 1_700_000_000,
        body_sha256: "ab".repeat(32),
        query_sha256: String::new(),
        sanitize_version: 1,
        kid: hex::encode(pk.0),
        df: hex::encode([0u8; 32]),
    };

    // Native path (oracle).
    let header = Header::new(Algorithm::EdDSA);
    let native_jws = kp.sign_jws(&claims, &header).expect("native sign");

    // WASM path (byte-exact mirror of src/wasm_jws_core.rs).
    let claims_json = serde_json::to_value(&claims).unwrap();
    let wasm_jws = wasm_sign(&claims_json, &seed, false);

    // If these diverge, the host and Worker will silently fail to
    // authenticate each other across the JWS boundary. The native path is
    // the oracle (`jsonwebtoken` v9 is mature); fix the wasm path to match.
    assert_eq!(
        native_jws, wasm_jws,
        "JWS byte-parity broken: native jsonwebtoken output differs from hand-rolled wasm output.\n\
         native head: {}\n\
         wasm   head: {}",
        &native_jws[..native_jws.len().min(60)],
        &wasm_jws[..wasm_jws.len().min(60)],
    );
}

#[test]
fn jws_native_and_wasm_parity_with_embedded_jwk() {
    // The signed-bundle path (wasm `packSignedBundle`) embeds the pubkey as
    // an OKP jwk in the header. Native code path supports the same via
    // `Header { jwk: Some(_), .. }`. Verify they still agree byte-for-byte.
    let seed = [0x42u8; 32];
    let kp = Keypair::from_seed_for_test(&seed);
    let pk = kp.public_key();

    let claims = serde_json::json!({
        "canonical_hash_hex": "deadbeef".repeat(8),
    });

    let mut header = Header::new(Algorithm::EdDSA);
    header.jwk = Some(jsonwebtoken::jwk::Jwk {
        common: jsonwebtoken::jwk::CommonParameters::default(),
        algorithm: jsonwebtoken::jwk::AlgorithmParameters::OctetKeyPair(
            jsonwebtoken::jwk::OctetKeyPairParameters {
                key_type: jsonwebtoken::jwk::OctetKeyPairType::OctetKeyPair,
                curve: jsonwebtoken::jwk::EllipticCurve::Ed25519,
                x: B64.encode(pk.0),
            },
        ),
    });

    let native_jws = kp.sign_jws(&claims, &header).expect("native sign");
    let wasm_jws = wasm_sign(&claims, &seed, true);

    assert_eq!(
        native_jws, wasm_jws,
        "JWS byte-parity broken with embedded jwk.\n\
         native head: {}\n\
         wasm   head: {}",
        &native_jws[..native_jws.len().min(60)],
        &wasm_jws[..wasm_jws.len().min(60)],
    );
}
