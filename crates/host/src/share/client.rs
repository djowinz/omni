//! Thin wrapper around `reqwest::Client` — the single seam where
//! `omni_identity::sign_http_jws` attaches `Authorization: Omni-JWS <compact>`.
//!
//! Do not strip this wrapper "for simplicity" in a later pass; scattering signing
//! across call-sites is the anti-pattern this file exists to prevent.

use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use backon::ExponentialBuilder;
use base64::Engine;
use omni_guard_trait::Guard;
use omni_identity::{sign_http_jws, HttpJwsClaims, Keypair};
use reqwest::{Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use url::Url;

use super::cache::{ArtifactCache, ArtifactDetail};
use super::error::{UploadError, WorkerErrorKind};
use super::progress::UploadProgress;
use super::upload::{PackResult, UploadResult, UploadStatus};

/// Compile-time sanitize-pipeline version (spec §4 claim value). Bump with
/// every sanitize pipeline change; must match the Worker-side expectation.
pub const SANITIZE_VERSION: u32 = 1;

/// SHA-256 of the empty string (precomputed — hot path for every signed
/// request with no query/body).
const EMPTY_SHA256_HEX: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// TTL for cached `config_limits` response. Server limits change rarely.
const LIMITS_CACHE_TTL: Duration = Duration::from_secs(300);

pub struct ShareClient {
    base_url: Url,
    http: reqwest::Client,
    identity: Arc<Keypair>,
    guard: Arc<dyn Guard>,
    kid_b64: String,
    pubkey_hex: String,
    cache: ArtifactCache,
    retry_policy: ExponentialBuilder,
    limits_cache: AsyncMutex<Option<(Instant, omni_bundle::BundleLimits)>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListParams {
    pub kind: Option<String>,
    pub sort: Option<String>,
    pub tag: Vec<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListResult {
    pub items: Vec<ArtifactDetail>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PatchEdit {
    pub manifest: Option<serde_json::Value>,
    pub bundle_bytes: Option<Vec<u8>>,
    pub thumbnail_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportBody {
    pub category: String,
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VocabDoc {
    pub tags: Vec<String>,
    pub version: u32,
}

/// Server response for `POST /v1/upload` and `PATCH /v1/artifact/:id`.
#[derive(Debug, Clone, Deserialize)]
struct UploadResponseBody {
    artifact_id: String,
    content_hash: String,
    r2_url: String,
    thumbnail_url: String,
    #[serde(default)]
    created_at: i64,
    status: String,
}

/// Worker 4xx/5xx body per worker-api §3.
#[derive(Debug, Clone, Deserialize)]
struct ErrorBody {
    error: ErrorInner,
}

#[derive(Debug, Clone, Deserialize)]
struct ErrorInner {
    code: String,
    message: String,
    #[serde(default)]
    retry_after: Option<u64>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    detail: Option<String>,
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
        let pk = identity.public_key().0;
        let kid_b64 = base64::engine::general_purpose::STANDARD.encode(pk);
        let pubkey_hex = hex::encode(pk);
        Self {
            base_url,
            http: reqwest::Client::builder()
                .user_agent(concat!("omni-host/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("reqwest client"),
            identity,
            guard,
            kid_b64,
            pubkey_hex,
            cache: ArtifactCache::new(),
            retry_policy,
            limits_cache: AsyncMutex::new(None),
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

        let body_sha = if body.is_empty() {
            EMPTY_SHA256_HEX.to_string()
        } else {
            hex::encode(Sha256::digest(body))
        };
        let query_sha = if query.is_empty() {
            EMPTY_SHA256_HEX.to_string()
        } else {
            hex::encode(Sha256::digest(query.as_bytes()))
        };

        let claims = HttpJwsClaims::new(
            self.kid_b64.clone(),
            base64::engine::general_purpose::STANDARD.encode(device_id.0),
            ts,
            method.as_str(),
            path,
            query_sha,
            body_sha,
            SANITIZE_VERSION,
        );

        let compact =
            sign_http_jws(&self.identity, &claims).map_err(|e| UploadError::Integrity {
                msg: "JWS signing failed".into(),
                source: Some(Box::new(e)),
            })?;

        Ok(builder
            .header("Authorization", format!("Omni-JWS {compact}"))
            .header("X-Omni-Version", env!("CARGO_PKG_VERSION"))
            .header("X-Omni-Sanitize-Version", SANITIZE_VERSION.to_string()))
    }

    pub(crate) fn url(&self, path: &str) -> Url {
        self.base_url.join(path).expect("valid path")
    }

    fn pubkey_hex(&self) -> &str {
        &self.pubkey_hex
    }

    /// Sign + send a JSON-body-less request, retrying transient failures, decoding a
    /// JSON response of type `T` on success or a structured error body on failure.
    async fn send_signed<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        query: &str,
        builder: RequestBuilder,
    ) -> Result<T, UploadError> {
        let builder = self.sign(&method, path, query, &[], builder)?;
        let resp = self.send_with_retry(builder).await?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        resp.json::<T>().await.map_err(UploadError::Network)
    }

    /// Sign + send; on success, drain the body and return `()`.
    async fn send_signed_empty(
        &self,
        method: Method,
        path: &str,
        body: &[u8],
        builder: RequestBuilder,
    ) -> Result<(), UploadError> {
        let builder = self.sign(&method, path, "", body, builder)?;
        let resp = self.send_with_retry(builder).await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(Self::decode_error(resp).await)
        }
    }

    pub async fn upload(
        &self,
        pack: PackResult,
        progress: mpsc::Sender<UploadProgress>,
    ) -> Result<UploadResult, UploadError> {
        let _ = progress.send(UploadProgress::Signing).await;

        let total = pack.sanitized_bytes.len() as u64;
        let _ = progress
            .send(UploadProgress::Uploading { sent: 0, total })
            .await;

        let manifest_json =
            serde_json::to_vec(&pack.manifest).map_err(|e| UploadError::BadInput {
                msg: "manifest serialization failed".into(),
                source: Some(Box::new(e)),
            })?;

        // Move PackResult fields directly into the multipart body; no clones.
        let PackResult {
            sanitized_bytes,
            thumbnail_png,
            manifest_name,
            manifest_kind,
            ..
        } = pack;

        let parts = vec![
            MultipartPart {
                name: "manifest",
                filename: "manifest.json",
                content_type: "application/json",
                bytes: manifest_json,
            },
            MultipartPart {
                name: "bundle",
                filename: "bundle.omnipkg",
                content_type: "application/octet-stream",
                bytes: sanitized_bytes,
            },
            MultipartPart {
                name: "thumbnail",
                filename: "thumbnail.png",
                content_type: "image/png",
                bytes: thumbnail_png,
            },
        ];
        let (body_bytes, content_type) = serialize_multipart(parts);

        let path = "/v1/upload";
        // Worker hashes the exact transmitted bytes, so we hand-assemble the
        // multipart body and sign `body_sha256` over the same bytes we ship.
        let builder = self
            .http
            .post(self.url(path))
            .header("Content-Type", content_type)
            .body(body_bytes.clone());
        let builder = self.sign(&Method::POST, path, "", &body_bytes, builder)?;

        let resp = self.send_with_retry(builder).await?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        let body: UploadResponseBody = resp.json().await.map_err(UploadError::Network)?;
        let result = UploadResult {
            artifact_id: body.artifact_id.clone(),
            content_hash: body.content_hash.clone(),
            r2_url: body.r2_url.clone(),
            thumbnail_url: body.thumbnail_url.clone(),
            status: UploadStatus::from_worker(&body.status),
        };

        let detail = ArtifactDetail {
            artifact_id: body.artifact_id.clone(),
            content_hash: body.content_hash,
            author_pubkey: self.pubkey_hex.clone(),
            name: manifest_name,
            kind: manifest_kind,
            r2_url: body.r2_url,
            thumbnail_url: body.thumbnail_url,
            updated_at: body.created_at,
        };
        self.cache
            .insert((self.pubkey_hex.clone(), body.artifact_id.clone()), detail)
            .await;

        Ok(result)
    }

    pub async fn list(&self, params: ListParams) -> Result<ListResult, UploadError> {
        let mut url = self.url("/v1/list");
        {
            let mut q = url.query_pairs_mut();
            if let Some(k) = &params.kind {
                q.append_pair("kind", k);
            }
            if let Some(s) = &params.sort {
                q.append_pair("sort", s);
            }
            for t in &params.tag {
                q.append_pair("tag", t);
            }
            if let Some(c) = &params.cursor {
                q.append_pair("cursor", c);
            }
            if let Some(l) = params.limit {
                q.append_pair("limit", &l.to_string());
            }
        }
        let query = url.query().unwrap_or("").to_string();
        let mut lr: ListResult = self
            .send_signed(Method::GET, "/v1/list", &query, self.http.get(url))
            .await?;
        lr.items = self
            .cache
            .merge_into_list(self.pubkey_hex(), lr.items)
            .await;
        Ok(lr)
    }

    pub async fn patch(&self, id: &str, edit: PatchEdit) -> Result<UploadResult, UploadError> {
        let mut parts: Vec<MultipartPart> = Vec::new();
        if let Some(m) = edit.manifest {
            let text = serde_json::to_vec(&m).map_err(|e| UploadError::BadInput {
                msg: "patch manifest serialization failed".into(),
                source: Some(Box::new(e)),
            })?;
            parts.push(MultipartPart {
                name: "manifest",
                filename: "manifest.json",
                content_type: "application/json",
                bytes: text,
            });
        }
        if let Some(b) = edit.bundle_bytes {
            parts.push(MultipartPart {
                name: "bundle",
                filename: "bundle.omnipkg",
                content_type: "application/octet-stream",
                bytes: b,
            });
        }
        if let Some(t) = edit.thumbnail_bytes {
            parts.push(MultipartPart {
                name: "thumbnail",
                filename: "thumbnail.png",
                content_type: "image/png",
                bytes: t,
            });
        }
        let (body_bytes, content_type) = serialize_multipart(parts);
        let path = format!("/v1/artifact/{id}");
        let builder = self
            .http
            .patch(self.url(&path))
            .header("Content-Type", content_type)
            .body(body_bytes.clone());
        let builder = self.sign(&Method::PATCH, &path, "", &body_bytes, builder)?;
        let resp = self.send_with_retry(builder).await?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        let body: UploadResponseBody = resp.json().await.map_err(UploadError::Network)?;
        Ok(UploadResult {
            artifact_id: body.artifact_id,
            content_hash: body.content_hash,
            r2_url: body.r2_url,
            thumbnail_url: body.thumbnail_url,
            status: UploadStatus::Updated,
        })
    }

    pub async fn delete(&self, id: &str) -> Result<(), UploadError> {
        let path = format!("/v1/artifact/{id}");
        self.send_signed_empty(
            Method::DELETE,
            &path,
            &[],
            self.http.delete(self.url(&path)),
        )
        .await
    }

    pub async fn report(&self, id: &str, body: ReportBody) -> Result<(), UploadError> {
        #[derive(Serialize)]
        struct Wire<'a> {
            artifact_id: &'a str,
            category: &'a str,
            note: &'a str,
        }
        let raw = serde_json::to_vec(&Wire {
            artifact_id: id,
            category: &body.category,
            note: &body.note,
        })
        .map_err(|e| UploadError::BadInput {
            msg: "report body serialization failed".into(),
            source: Some(Box::new(e)),
        })?;
        let path = "/v1/report";
        let builder = self
            .http
            .post(self.url(path))
            .header("Content-Type", "application/json")
            .body(raw.clone());
        self.send_signed_empty(Method::POST, path, &raw, builder)
            .await
    }

    pub async fn gallery(&self) -> Result<Vec<ArtifactDetail>, UploadError> {
        let path = "/v1/me/gallery";
        let lr: ListResult = self
            .send_signed(Method::GET, path, "", self.http.get(self.url(path)))
            .await?;
        Ok(self
            .cache
            .merge_into_list(self.pubkey_hex(), lr.items)
            .await)
    }

    pub async fn config_vocab(&self) -> Result<VocabDoc, UploadError> {
        let path = "/v1/config/vocab";
        self.send_signed(Method::GET, path, "", self.http.get(self.url(path)))
            .await
    }

    pub async fn config_limits(&self) -> Result<omni_bundle::BundleLimits, UploadError> {
        // Small TTL cache — server limits change rarely.
        {
            let guard = self.limits_cache.lock().await;
            if let Some((when, limits)) = *guard {
                if when.elapsed() < LIMITS_CACHE_TTL {
                    return Ok(limits);
                }
            }
        }
        let path = "/v1/config/limits";
        #[derive(Deserialize)]
        struct LimitsBody {
            max_bundle_compressed: u64,
            max_bundle_uncompressed: u64,
            max_entries: usize,
        }
        let b: LimitsBody = self
            .send_signed(Method::GET, path, "", self.http.get(self.url(path)))
            .await?;
        let limits = omni_bundle::BundleLimits {
            max_bundle_compressed: b.max_bundle_compressed,
            max_bundle_uncompressed: b.max_bundle_uncompressed,
            max_entries: b.max_entries,
        };
        *self.limits_cache.lock().await = Some((Instant::now(), limits));
        Ok(limits)
    }

    async fn send_with_retry(
        &self,
        builder: RequestBuilder,
    ) -> Result<reqwest::Response, UploadError> {
        use backon::BackoffBuilder;
        // Transient = network error (connect/timeout/request) OR HTTP 429 / 5xx response.
        // Spec §9: "429 with Retry-After, 5xx, connection reset" retry through backon.
        let mut backoff = self.retry_policy.build();
        loop {
            let b = builder.try_clone().ok_or_else(|| UploadError::BadInput {
                msg: "request not cloneable (streaming body)".into(),
                source: None,
            })?;
            match b.send().await {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    let transient = status == 429 || (500..=599).contains(&status);
                    if !transient {
                        return Ok(resp);
                    }
                    match backoff.next() {
                        Some(delay) => tokio::time::sleep(delay).await,
                        // Budget exhausted — surface the structured error from body.
                        None => return Ok(resp),
                    }
                }
                Err(e) => {
                    let transient = e.is_connect() || e.is_timeout() || e.is_request();
                    if !transient {
                        return Err(UploadError::Network(e));
                    }
                    match backoff.next() {
                        Some(delay) => tokio::time::sleep(delay).await,
                        None => return Err(UploadError::Network(e)),
                    }
                }
            }
        }
    }

    async fn decode_error(resp: reqwest::Response) -> UploadError {
        let status = resp.status().as_u16();
        let header_retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs);
        let body = resp.json::<ErrorBody>().await.ok();
        let (code, message, kind_str, detail, retry_after) = match body {
            Some(b) => (
                b.error.code,
                b.error.message,
                b.error
                    .kind
                    .unwrap_or_else(|| default_kind_for_status(status).into()),
                b.error.detail,
                b.error
                    .retry_after
                    .map(Duration::from_secs)
                    .or(header_retry_after),
            ),
            None => (
                "UNKNOWN".into(),
                format!("HTTP {status}"),
                default_kind_for_status(status).into(),
                None,
                header_retry_after,
            ),
        };
        let kind = kind_str.parse().unwrap_or(WorkerErrorKind::Io);
        UploadError::ServerReject {
            status,
            code,
            kind,
            detail,
            message,
            retry_after,
        }
    }
}

impl FromStr for WorkerErrorKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "Malformed" => Self::Malformed,
            "Unsafe" => Self::Unsafe,
            "Integrity" => Self::Integrity,
            "Auth" => Self::Auth,
            "Quota" => Self::Quota,
            "Admin" => Self::Admin,
            "Io" => Self::Io,
            _ => return Err(()),
        })
    }
}

fn default_kind_for_status(status: u16) -> &'static str {
    match status {
        400 | 404 | 409 | 413 => "Malformed",
        401 | 403 | 426 => "Auth",
        422 => "Unsafe",
        410 => "Integrity",
        428 | 429 => "Quota",
        _ => "Io",
    }
}

/// Single hand-assembled multipart/form-data part.
pub(crate) struct MultipartPart {
    pub name: &'static str,
    pub filename: &'static str,
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Hand-assemble an RFC 7578 multipart/form-data body.
///
/// The Worker hashes the exact transmitted bytes to verify `body_sha256`, so we
/// must be able to compute the hash over the same bytes we ship. Building the
/// body ourselves (rather than streaming through `reqwest::multipart::Form`)
/// guarantees that equivalence. Boundary is a process-local time+counter string.
pub(crate) fn serialize_multipart(parts: Vec<MultipartPart>) -> (Vec<u8>, String) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let boundary = format!("omni-{:032x}-{:016x}", nanos, n);

    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(b"--");
        out.extend_from_slice(boundary.as_bytes());
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(
            format!(
                "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                p.name, p.filename
            )
            .as_bytes(),
        );
        out.extend_from_slice(format!("Content-Type: {}\r\n\r\n", p.content_type).as_bytes());
        out.extend_from_slice(&p.bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"--");
    out.extend_from_slice(boundary.as_bytes());
    out.extend_from_slice(b"--\r\n");

    let content_type = format!("multipart/form-data; boundary={boundary}");
    (out, content_type)
}
