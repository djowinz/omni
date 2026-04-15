//! Thin wrapper around `reqwest::Client` — the single seam where
//! `omni_identity::sign_http_jws` attaches `Authorization: Omni-JWS <compact>`.
//!
//! Do not strip this wrapper "for simplicity" in a later pass; scattering signing
//! across call-sites is the anti-pattern this file exists to prevent
//! (spec §4; retro-005 D3).

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use backon::ExponentialBuilder;
use base64::Engine;
use omni_guard_trait::Guard;
use omni_identity::{sign_http_jws, HttpJwsClaims, Keypair};
use reqwest::{Method, RequestBuilder};
use sha2::{Digest, Sha256};
use url::Url;

use super::cache::ArtifactCache;
use super::error::UploadError;

/// Compile-time sanitize-pipeline version (spec §4 claim value). Bump with
/// every sanitize pipeline change; must match the Worker-side expectation.
pub const SANITIZE_VERSION: u32 = 1;

pub struct ShareClient {
    base_url: Url,
    http: reqwest::Client,
    identity: Arc<Keypair>,
    guard: Arc<dyn Guard>,
    sanitize_version: u32,
    cache: ArtifactCache,
    retry_policy: ExponentialBuilder,
}

impl ShareClient {
    pub fn new(base_url: Url, identity: Arc<Keypair>, guard: Arc<dyn Guard>) -> Self {
        Self::with_policy(base_url, identity, guard, ExponentialBuilder::default())
    }

    pub fn with_policy(
        base_url: Url,
        identity: Arc<Keypair>,
        guard: Arc<dyn Guard>,
        retry_policy: ExponentialBuilder,
    ) -> Self {
        Self {
            base_url,
            http: reqwest::Client::builder()
                .user_agent(concat!("omni-host/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
            identity,
            guard,
            sanitize_version: SANITIZE_VERSION,
            cache: ArtifactCache::new(),
            retry_policy,
        }
    }

    pub fn cache(&self) -> &ArtifactCache {
        &self.cache
    }

    /// Sign a request and attach `Authorization: Omni-JWS <compact>` +
    /// `X-Omni-Version` + `X-Omni-Sanitize-Version`. Every client method funnels through this.
    pub(crate) fn sign(
        &self,
        method: &Method,
        path: &str,
        query: &str,
        body: &[u8],
        builder: RequestBuilder,
    ) -> Result<RequestBuilder, UploadError> {
        let device_id = self.guard.device_id().map_err(|e| UploadError::BadInput {
            msg: "device fingerprint unavailable".into(),
            source: Some(Box::new(e)),
        })?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let body_sha = hex::encode(Sha256::digest(body));
        let query_sha = hex::encode(Sha256::digest(query.as_bytes()));

        let claims = HttpJwsClaims::new(
            base64::engine::general_purpose::STANDARD.encode(self.identity.public_key().0),
            base64::engine::general_purpose::STANDARD.encode(device_id.0),
            ts,
            method.as_str(),
            path,
            query_sha,
            body_sha,
            self.sanitize_version,
        );

        let compact = sign_http_jws(&self.identity, &claims).map_err(|e| UploadError::Integrity {
            msg: "JWS signing failed".into(),
            source: Some(Box::new(e)),
        })?;

        Ok(builder
            .header("Authorization", format!("Omni-JWS {compact}"))
            .header("X-Omni-Version", env!("CARGO_PKG_VERSION"))
            .header("X-Omni-Sanitize-Version", self.sanitize_version.to_string()))
    }

    pub(crate) fn url(&self, path: &str) -> Url {
        self.base_url.join(path).expect("valid path")
    }
}
