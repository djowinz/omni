//! HTTP-request JWS envelope per worker-api.md §2.
//!
//! Keeps HTTP-envelope canonicalization inside omni-identity (architectural
//! invariant #1 — single signing authority). Callers supply method/path/body
//! hashes; they never assemble a canonical string.

use jsonwebtoken::{Algorithm, Header};
use serde::{Deserialize, Serialize};

use crate::{IdentityError, Keypair};

/// Protected-header claims for `Authorization: Omni-JWS <compact>`.
///
/// Field order + field names MUST match `contracts/worker-api.md` §2 exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpJwsClaims {
    pub alg: String,          // always "EdDSA"
    pub crv: String,          // always "Ed25519"
    pub typ: String,          // always "Omni-HTTP-JWS"
    pub kid: String,          // base64(pubkey, 32 bytes)
    pub df: String,           // base64(device fingerprint, 32 bytes)
    pub ts: i64,              // seconds since epoch
    pub method: String,       // uppercase HTTP method
    pub path: String,         // request path, leading slash
    pub query_sha256: String, // hex SHA-256 of raw query string ("" if none)
    pub body_sha256: String,  // hex SHA-256 of raw body bytes ("" if none)
    pub sanitize_version: u32,
}

impl HttpJwsClaims {
    // The 8-field envelope is a wire contract (worker-api §2), not a refactor candidate.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kid_b64: String,
        df_b64: String,
        ts: i64,
        method: &str,
        path: &str,
        query_sha256_hex: String,
        body_sha256_hex: String,
        sanitize_version: u32,
    ) -> Self {
        Self {
            alg: "EdDSA".to_string(),
            crv: "Ed25519".to_string(),
            typ: "Omni-HTTP-JWS".to_string(),
            kid: kid_b64,
            df: df_b64,
            ts,
            method: method.to_ascii_uppercase(),
            path: path.to_string(),
            query_sha256: query_sha256_hex,
            body_sha256: body_sha256_hex,
            sanitize_version,
        }
    }
}

/// Sign an HTTP-request JWS envelope with `keypair`. Returns the compact JWS
/// intended for `Authorization: Omni-JWS <compact>`.
///
/// Delegates to [`Keypair::sign_jws`]; forces `alg = EdDSA` and `typ = "Omni-HTTP-JWS"`.
pub fn sign_http_jws(keypair: &Keypair, claims: &HttpJwsClaims) -> Result<String, IdentityError> {
    let mut header = Header::new(Algorithm::EdDSA);
    header.typ = Some("Omni-HTTP-JWS".to_string());
    keypair.sign_jws(claims, &header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Keypair;

    fn sample() -> HttpJwsClaims {
        HttpJwsClaims::new(
            "AAAA".into(),
            "BBBB".into(),
            1_760_000_000,
            "post",
            "/v1/upload",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
            "deadbeef".into(),
            1,
        )
    }

    #[test]
    fn method_is_uppercased() {
        let c = sample();
        assert_eq!(c.method, "POST");
    }

    #[test]
    fn roundtrip_signs_and_verifies() {
        let kp = Keypair::generate();
        let jws = sign_http_jws(&kp, &sample()).expect("sign");
        // compact JWS has three base64url segments
        assert_eq!(jws.matches('.').count(), 2);
        // claims decode via the public verify_jws helper
        let verified = crate::verify_jws::<HttpJwsClaims>(&jws, &kp.public_key()).expect("verify");
        assert_eq!(verified.claims.method, "POST");
        assert_eq!(verified.claims.typ, "Omni-HTTP-JWS");
    }

    #[test]
    fn mutated_signature_fails_verify() {
        let kp = Keypair::generate();
        let jws = sign_http_jws(&kp, &sample()).unwrap();
        // flip last char of signature segment
        let mut bytes: Vec<char> = jws.chars().collect();
        let last = bytes.len() - 1;
        bytes[last] = if bytes[last] == 'A' { 'B' } else { 'A' };
        let tampered: String = bytes.into_iter().collect();
        let err = crate::verify_jws::<HttpJwsClaims>(&tampered, &kp.public_key());
        assert!(err.is_err());
    }
}
