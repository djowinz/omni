//! Shared reqwest client + contract kind -> CLI exit-code mapper.
//!
//! Every admin subcommand (T13+) builds an [`AdminClient`] and calls
//! [`AdminClient::send_signed`]. The method signs the request with the
//! operator keypair (see [`crate::auth`]) and attaches the JWS in the
//! `Authorization: Omni-JWS <compact>` header. The `extra_headers` slice
//! allows per-call headers (e.g. T19's `X-Omni-Admin-Force: 1`) without a
//! special-case method.

use crate::auth::{self, HttpClaims};
use omni_identity::Keypair;
use serde::de::DeserializeOwned;

pub struct AdminClient {
    pub base: String,
    pub kp: Keypair,
    pub http: reqwest::Client,
    pub json_mode: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AdminError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("signing: {0}")]
    Signing(#[from] anyhow::Error),
    #[error("HTTP {status}: {body}")]
    Response {
        status: reqwest::StatusCode,
        kind: String,
        detail: Option<String>,
        body: String,
    },
    #[error("decode: {0}")]
    Decode(#[from] serde_json::Error),
}

impl AdminClient {
    pub fn new(base: impl Into<String>, kp: Keypair, json_mode: bool) -> Self {
        Self {
            base: base.into(),
            kp,
            http: reqwest::Client::builder()
                .user_agent(concat!("omni-admin/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
            json_mode,
        }
    }

    pub async fn send_signed<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        query: Option<&str>,
        body: Option<&[u8]>,
        extra_headers: &[(&str, &str)],
    ) -> Result<T, AdminError> {
        let q = query.unwrap_or("").as_bytes();
        let b = body.unwrap_or(&[]);
        let claims = HttpClaims::new(&self.kp, method.as_str(), path, q, b);
        let jws = auth::sign_claims(&self.kp, &claims)?;
        let url = format!(
            "{}{}{}",
            self.base.trim_end_matches('/'),
            path,
            query.map(|q| format!("?{q}")).unwrap_or_default(),
        );
        let mut req = self
            .http
            .request(method, &url)
            .header("Authorization", format!("Omni-JWS {jws}"))
            .header("X-Omni-Version", env!("CARGO_PKG_VERSION"))
            .header("X-Omni-Sanitize-Version", "1");
        for (k, v) in extra_headers {
            req = req.header(*k, *v);
        }
        if let Some(b) = body {
            req = req
                .body(b.to_vec())
                .header("Content-Type", "application/json");
        }
        let resp = req.send().await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let body_str = String::from_utf8_lossy(&bytes).to_string();
            let (kind, detail) = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| {
                    let err = v.get("error")?;
                    let kind = err.get("kind")?.as_str()?.to_string();
                    let detail = err.get("detail").and_then(|d| d.as_str()).map(String::from);
                    Some((kind, detail))
                })
                .unwrap_or_else(|| ("Io".to_string(), None));
            return Err(AdminError::Response {
                status,
                kind,
                detail,
                body: body_str,
            });
        }
        Ok(serde_json::from_slice(&bytes)?)
    }
}

/// Map contract error kind -> CLI exit code per spec §6.
pub fn kind_to_exit_code(kind: &str) -> i32 {
    match kind {
        "Admin" => 2,
        "Auth" => 3,
        "Malformed" | "Integrity" => 4,
        "Io" => 5,
        "Quota" => 6,
        _ => 1,
    }
}
