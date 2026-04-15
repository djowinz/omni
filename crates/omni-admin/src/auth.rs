//! HTTP JWS signing for the omni-admin CLI.
//!
//! Claim shape follows worker-api.md §2 (authoritative). `kid` + `df` are
//! base64url-nopad (RFC 7515 standard); `query_sha256` + `body_sha256` are hex.
//!
//! NOTE: the #012 sub-spec showed `hex::encode(...)` for `kid`, but the
//! contract (worker-api.md §2) is authoritative and specifies base64url-nopad
//! — the encoding every JWS library uses natively. Worker-side (#008 JWS
//! verifier) converts the base64 `kid` to lowercase hex before matching
//! against `OMNI_ADMIN_PUBKEYS`; that conversion is not our concern here.

use base64::Engine;
use omni_identity::Keypair;
use serde::Serialize;
use sha2::{Digest, Sha256};

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

#[derive(Serialize, Debug, Clone)]
pub struct HttpClaims {
    pub alg: &'static str,
    pub crv: &'static str,
    pub typ: &'static str,
    /// base64url-nopad pubkey (32B -> 43 chars)
    pub kid: String,
    /// base64url-nopad device fingerprint (32B -> 43 chars)
    pub df: String,
    pub ts: i64,
    pub method: String,
    pub path: String,
    /// hex-encoded SHA-256 of the raw query string bytes (no leading `?`).
    pub query_sha256: String,
    /// hex-encoded SHA-256 of the raw request body bytes.
    pub body_sha256: String,
    pub sanitize_version: u32,
}

impl HttpClaims {
    pub fn new(kp: &Keypair, method: &str, path: &str, query: &[u8], body: &[u8]) -> Self {
        Self {
            alg: "EdDSA",
            crv: "Ed25519",
            typ: "Omni-HTTP-JWS",
            kid: b64(&kp.public_key().0),
            df: b64(&machine_guid_sha256()),
            ts: chrono::Utc::now().timestamp(),
            method: method.to_string(),
            path: path.to_string(),
            query_sha256: hex::encode(Sha256::digest(query)),
            body_sha256: hex::encode(Sha256::digest(body)),
            sanitize_version: 1,
        }
    }
}

pub fn sign_claims(kp: &Keypair, claims: &HttpClaims) -> anyhow::Result<String> {
    let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::EdDSA);
    Ok(kp.sign_jws(claims, &header)?)
}

/// SHA-256 of the machine identifier. Windows: `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`.
/// Linux/macOS: `/etc/machine-id` (falls back to empty string if unreadable).
fn machine_guid_sha256() -> [u8; 32] {
    Sha256::digest(read_machine_guid().as_bytes()).into()
}

#[cfg(windows)]
fn read_machine_guid() -> String {
    use windows::core::w;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
        KEY_WOW64_64KEY, REG_SZ, REG_VALUE_TYPE,
    };
    unsafe {
        let mut hk = HKEY::default();
        let sub = w!("SOFTWARE\\Microsoft\\Cryptography");
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, sub, 0, KEY_READ | KEY_WOW64_64KEY, &mut hk).is_err()
        {
            return String::new();
        }
        let mut ty = REG_VALUE_TYPE::default();
        let mut buf = [0u16; 128];
        let mut sz = (buf.len() * 2) as u32;
        let name = w!("MachineGuid");
        let r = RegQueryValueExW(
            hk,
            name,
            None,
            Some(&mut ty),
            Some(buf.as_mut_ptr() as *mut u8),
            Some(&mut sz),
        );
        let _ = RegCloseKey(hk);
        if r.is_err() || ty != REG_SZ {
            return String::new();
        }
        let len = (sz as usize).saturating_sub(2) / 2; // drop trailing NUL
        String::from_utf16_lossy(&buf[..len])
    }
}

#[cfg(not(windows))]
fn read_machine_guid() -> String {
    std::fs::read_to_string("/etc/machine-id")
        .or_else(|_| std::fs::read_to_string("/var/lib/dbus/machine-id"))
        .unwrap_or_default()
        .trim()
        .to_string()
}
