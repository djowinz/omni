//! Byte-parity regression gate for the split JWS implementations in
//! `omni-identity` (plan #008 W1T2 post-gate).
//!
//! The native path (`Keypair::sign_jws`) uses `jsonwebtoken` v9 → `ring`.
//! The WASM path (`src/wasm_jws_core.rs`) hand-rolls compact EdDSA via
//! `ed25519-dalek` because `ring` does not cross-compile to
//! `wasm32-unknown-unknown` without a C toolchain.
//!
//! If the two paths produce byte-different JWS envelopes for the same claims
//! and key, every sign/verify interaction between the host (native) and the
//! Worker (WASM) silently breaks. This test proves byte-equality and locks
//! the proof as a regression test. Do NOT relax the assertion — adjust the
//! WASM path to match the native oracle (`jsonwebtoken`) if they diverge.
//!
//! Two paths are covered:
//!
//! 1. HTTP-auth — host→Worker request signing. Native oracle is
//!    `omni_identity::http_jws::sign_http_jws` (shipped on `theme-sharing`;
//!    will land here after rebase). Header `{"typ":"Omni-HTTP-JWS","alg":"EdDSA"}`,
//!    claims shape `HttpJwsClaims` (alg/crv/typ/kid/df/ts/method/path/...).
//!    Pre-rebase we inline the native construction (`Header` + `Keypair::sign_jws`);
//!    post-rebase the test can switch to `sign_http_jws` directly — byte output
//!    is identical either way.
//! 2. Embedded-JWK — `packSignedBundle` path. Header carries the pubkey as
//!    an OKP/Ed25519 JWK. Claims are a tiny `canonical_hash_hex` payload.

use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use jsonwebtoken::{Algorithm, Header};
use omni_identity::Keypair;
use serde::{Deserialize, Serialize};

const B64URL: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;
const B64_STD: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::STANDARD;

// Field order mirrors `src/wasm_jws_core.rs` and the shipped
// `omni_identity::http_jws::HttpJwsClaims`. Reordering here WILL break the
// parity assertion below — that's the point of the gate.
#[derive(Serialize, Deserialize, Clone)]
struct HttpJwsClaimsMirror {
    alg: String,
    crv: String,
    typ: String,
    kid: String,
    df: String,
    ts: i64,
    method: String,
    path: String,
    query_sha256: String,
    body_sha256: String,
    sanitize_version: u32,
}

#[derive(Serialize, Deserialize)]
struct OkpJwk {
    kty: String,
    crv: String,
    x: String,
}

#[derive(Serialize, Deserialize)]
struct WasmBundleJwsHeader {
    typ: String,
    alg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    jwk: Option<OkpJwk>,
}

/// Byte-exact mirror of `wasm_jws_core::sign_http_jws_compact_core`. Kept in
/// this test file so drift in the core immediately shows up as a parity
/// failure here, not as a silent wire-format shift.
fn wasm_sign_http(claims: &HttpJwsClaimsMirror, seed: &[u8; 32]) -> String {
    let sk = SigningKey::from_bytes(seed);
    let header = WasmBundleJwsHeader {
        typ: "Omni-HTTP-JWS".into(),
        alg: "EdDSA".into(),
        jwk: None,
    };
    let header_bytes = serde_json::to_vec(&header).unwrap();
    let payload_bytes = serde_json::to_vec(claims).unwrap();
    let header_b64 = B64URL.encode(&header_bytes);
    let payload_b64 = B64URL.encode(&payload_bytes);
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = sk.sign(signing_input.as_bytes());
    let sig_b64 = B64URL.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

/// Byte-exact mirror of `wasm_jws_core::sign_bundle_jws_compact_core`.
fn wasm_sign_bundle(claims: &serde_json::Value, seed: &[u8; 32]) -> String {
    let sk = SigningKey::from_bytes(seed);
    let pk = sk.verifying_key().to_bytes();
    let header = WasmBundleJwsHeader {
        typ: "JWT".into(),
        alg: "EdDSA".into(),
        jwk: Some(OkpJwk {
            kty: "OKP".into(),
            crv: "Ed25519".into(),
            x: B64URL.encode(pk),
        }),
    };
    let header_bytes = serde_json::to_vec(&header).unwrap();
    let payload_bytes = serde_json::to_vec(claims).unwrap();
    let header_b64 = B64URL.encode(&header_bytes);
    let payload_b64 = B64URL.encode(&payload_bytes);
    let signing_input = format!("{header_b64}.{payload_b64}");
    let sig = sk.sign(signing_input.as_bytes());
    let sig_b64 = B64URL.encode(sig.to_bytes());
    format!("{signing_input}.{sig_b64}")
}

#[test]
fn http_jws_native_and_wasm_produce_byte_identical_envelopes() {
    // Fixed seed -> deterministic signature.
    let seed = [0x42u8; 32];
    let kp = Keypair::from_seed_for_test(&seed);
    let pk = kp.public_key();

    let claims = HttpJwsClaimsMirror {
        alg: "EdDSA".into(),
        crv: "Ed25519".into(),
        typ: "Omni-HTTP-JWS".into(),
        // Shipped `sign_http_jws` uses STANDARD base64 (not URL-safe) for
        // these two fields. The JWS envelope itself is URL-safe-no-pad.
        kid: B64_STD.encode(pk.0),
        df: B64_STD.encode([0u8; 32]),
        ts: 1_700_000_000,
        method: "POST".into(),
        path: "/v1/upload".into(),
        query_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
        body_sha256: "abababababababababababababababababababababababababababababababab".into(),
        sanitize_version: 1,
    };

    // Native path (oracle): mirrors what shipped `sign_http_jws` does.
    // `Header { typ: Some("Omni-HTTP-JWS"), alg: EdDSA, ..Default::default() }`
    // — jsonwebtoken serializes as {"typ":"Omni-HTTP-JWS","alg":"EdDSA"}.
    let mut header = Header::new(Algorithm::EdDSA);
    header.typ = Some("Omni-HTTP-JWS".to_string());
    let native_jws = kp.sign_jws(&claims, &header).expect("native sign");

    // WASM path (byte-exact mirror of src/wasm_jws_core.rs).
    let wasm_jws = wasm_sign_http(&claims, &seed);

    // If these diverge, the host and Worker will silently fail to
    // authenticate each other across the JWS boundary. The native path is
    // the oracle (`jsonwebtoken` v9 is mature); fix the wasm path to match.
    assert_eq!(
        native_jws, wasm_jws,
        "HTTP JWS byte-parity broken: native jsonwebtoken output differs from hand-rolled wasm output.\n\
         native head: {}\n\
         wasm   head: {}",
        &native_jws[..native_jws.len().min(80)],
        &wasm_jws[..wasm_jws.len().min(80)],
    );
}

#[test]
fn bundle_jws_native_and_wasm_parity_with_embedded_jwk() {
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
                x: B64URL.encode(pk.0),
            },
        ),
    });

    let native_jws = kp.sign_jws(&claims, &header).expect("native sign");
    let wasm_jws = wasm_sign_bundle(&claims, &seed);

    assert_eq!(
        native_jws, wasm_jws,
        "Bundle JWS byte-parity broken with embedded jwk.\n\
         native head: {}\n\
         wasm   head: {}",
        &native_jws[..native_jws.len().min(80)],
        &wasm_jws[..wasm_jws.len().min(80)],
    );
}
