use admin::auth::{self, HttpClaims};
use identity::Keypair;

#[test]
fn signs_http_jws_with_admin_keypair() {
    let kp = Keypair::generate();
    let claims = HttpClaims::new(&kp, "GET", "/v1/admin/stats", b"", b"");
    let jws = auth::sign_claims(&kp, &claims).expect("sign");
    assert!(jws.starts_with("ey"));
    assert_eq!(jws.matches('.').count(), 2);
}

#[test]
fn claims_carry_contract_fields() {
    let kp = Keypair::generate();
    let claims = HttpClaims::new(&kp, "POST", "/v1/admin/pubkey/ban", b"a=1", b"{\"x\":1}");
    assert_eq!(claims.alg, "EdDSA");
    assert_eq!(claims.crv, "Ed25519");
    assert_eq!(claims.typ, "Omni-HTTP-JWS");
    assert_eq!(claims.method, "POST");
    assert_eq!(claims.path, "/v1/admin/pubkey/ban");
    // query_sha256 + body_sha256 are hex, 64 chars each
    assert_eq!(claims.query_sha256.len(), 64);
    assert_eq!(claims.body_sha256.len(), 64);
    // kid + df are base64url-nopad of 32 bytes -> 43 chars
    assert_eq!(claims.kid.len(), 43);
    assert_eq!(claims.df.len(), 43);
}
