//! Thin wrapper around `reqwest::Client` — the single seam where
//! `identity::sign_http_jws` attaches `Authorization: Omni-JWS <compact>`.
//!
//! Do not strip this wrapper "for simplicity" in a later pass; scattering signing
//! across call-sites is the anti-pattern this file exists to prevent.
//!
//! Also hosts the `download` method used by the install pipeline; see
//! [`ShareClient::download`] for the streaming GET of `/v1/download/:id`
//! (optional-auth per worker-api §4.2).

use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use backon::ExponentialBuilder;
use base64::Engine;
use identity::{sign_http_jws, HttpJwsClaims, Keypair};
use omni_guard_trait::Guard;
use reqwest::{Method, RequestBuilder};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;
use url::Url;

use super::cache::{ArtifactCache, CachedArtifactDetail};
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
    limits_cache: AsyncMutex<Option<(Instant, bundle::BundleLimits)>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListParams {
    pub kind: Option<String>,
    pub sort: Option<String>,
    pub tag: Vec<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
    /// Optional 64-hex author pubkey filter. When `Some`, the worker
    /// returns only that author's artifacts. Used by #015's My Uploads
    /// sub-tab, which passes the editor's own pubkey to show the
    /// current user's published work.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_pubkey: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ListResult {
    pub items: Vec<CachedArtifactDetail>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// Full artifact metadata as returned by `GET /v1/artifact/:id`
/// (worker-api.md §4.4).
///
/// Distinct from [`super::cache::CachedArtifactDetail`]: that type holds the
/// subset of fields the post-upload cache tracks for KV eventual-consistency
/// merging; this type mirrors the complete wire shape including `manifest`,
/// `reports`, `created_at`, `status`, etc. Editors consume this struct as the
/// payload of the `explorer.getResult` frame.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ArtifactDetail {
    pub artifact_id: String,
    pub kind: String,
    pub manifest: serde_json::Value,
    pub content_hash: String,
    pub r2_url: String,
    pub thumbnail_url: String,
    pub author_pubkey: String,
    pub author_fingerprint_hex: String,
    #[serde(default)]
    pub installs: u64,
    #[serde(default)]
    pub reports: u64,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
    pub status: String,
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

/// Upper bound on downloaded bytes. Defends against a hostile or buggy Worker
/// advertising a huge `Content-Length` (pre-alloc OOM) or streaming more bytes
/// than the bundle limits allow. 32 MiB — comfortably above `BundleLimits`'
/// uncompressed ceiling yet well below any host OOM threshold.
const MAX_DOWNLOAD_BYTES: u64 = 32 * 1024 * 1024;

/// Failure modes for [`ShareClient::download`]. Upload/install pipelines carve
/// richer domain errors atop these.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("http error: {0}")]
    Http(#[source] reqwest::Error),
    #[error("server returned status {status}: {body}")]
    Status { status: u16, body: String },
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

        let detail = CachedArtifactDetail {
            artifact_id: body.artifact_id.clone(),
            content_hash: body.content_hash,
            author_pubkey: self.pubkey_hex.clone(),
            name: manifest_name,
            kind: manifest_kind,
            r2_url: body.r2_url,
            thumbnail_url: body.thumbnail_url,
            updated_at: body.created_at,
            // author_fingerprint_hex is not returned by POST /v1/upload;
            // the detail-fetch path (/v1/artifact/:id) fills it later.
            author_fingerprint_hex: String::new(),
            tags: Vec::new(),
            installs: 0,
            created_at: body.created_at,
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
            if let Some(pk) = &params.author_pubkey {
                q.append_pair("author_pubkey", pk);
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

    /// `GET /v1/artifact/:id` — full artifact metadata (worker-api §4.4).
    ///
    /// Parallels [`ShareClient::list`] in request construction: signed
    /// GET through `send_signed`, server errors decoded into
    /// [`UploadError::ServerReject`] (`NOT_FOUND` / `TOMBSTONED` / etc.
    /// surface via the existing error mapping), network errors to
    /// `UploadError::Network`. Added in the phase-2 follow-up that wires
    /// `explorer.get`.
    pub async fn get_artifact(&self, artifact_id: &str) -> Result<ArtifactDetail, UploadError> {
        let path = format!("/v1/artifact/{artifact_id}");
        self.send_signed(Method::GET, &path, "", self.http.get(self.url(&path)))
            .await
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

    pub async fn gallery(&self) -> Result<Vec<CachedArtifactDetail>, UploadError> {
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

    pub async fn config_limits(&self) -> Result<bundle::BundleLimits, UploadError> {
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
        let limits = bundle::BundleLimits {
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

    /// Download the raw bundle bytes for `artifact_id` from `/v1/download/:id`.
    ///
    /// Progress is streamed through `on_chunk(received, total)` as each chunk
    /// arrives. The download endpoint is optional-auth per worker-api §4.2, so
    /// this issues an unsigned GET (JWS is attached only on signed routes).
    ///
    /// Returns a [`DownloadError`] carved from the HTTPS failure modes: network
    /// I/O (`Http`) vs. non-2xx status (`Status { status, body }`).
    pub async fn download<F: FnMut(u64, u64)>(
        &self,
        artifact_id: &str,
        mut on_chunk: F,
    ) -> Result<Vec<u8>, DownloadError> {
        use futures_util::StreamExt;
        let url = self
            .base_url
            .join(&format!("v1/download/{artifact_id}"))
            .expect("base_url is pre-validated; join cannot fail for static path");
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(DownloadError::Http)?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(DownloadError::Status {
                status: status.as_u16(),
                body,
            });
        }
        let total = resp.content_length().unwrap_or(0);
        // Cap pre-allocation: a hostile server advertising a huge Content-Length
        // must not tip us into an OOM before the first byte arrives.
        let preallocate = total.min(MAX_DOWNLOAD_BYTES) as usize;
        let mut buf = Vec::with_capacity(preallocate);
        let mut stream = resp.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(DownloadError::Http)?;
            buf.extend_from_slice(&chunk);
            if buf.len() as u64 > MAX_DOWNLOAD_BYTES {
                return Err(DownloadError::Status {
                    status: 0,
                    body: format!("download exceeded max bytes ({MAX_DOWNLOAD_BYTES})"),
                });
            }
            on_chunk(buf.len() as u64, total);
        }
        Ok(buf)
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

#[cfg(test)]
mod tests {
    use super::*;
    use omni_guard_trait::StubGuard;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn test_client(base: &str) -> ShareClient {
        ShareClient::new(
            Url::parse(base).unwrap(),
            Arc::new(Keypair::generate()),
            Arc::new(StubGuard) as Arc<dyn Guard>,
        )
    }

    #[tokio::test]
    async fn download_returns_bytes_and_reports_progress() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/download/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"hello".to_vec()))
            .mount(&server)
            .await;

        let client = test_client(&server.uri());
        let mut seen = 0u64;
        let bytes = client
            .download("abc", |rx, _total| {
                seen = rx;
            })
            .await
            .unwrap();
        assert_eq!(bytes, b"hello");
        assert_eq!(seen, 5);
    }

    #[tokio::test]
    async fn download_surfaces_non_200_as_status_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/download/missing"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
            .mount(&server)
            .await;
        let client = test_client(&server.uri());
        let err = client.download("missing", |_, _| {}).await.unwrap_err();
        match err {
            DownloadError::Status { status, .. } => assert_eq!(status, 404),
            _ => panic!("expected DownloadError::Status"),
        }
    }

    /// Canned §4.4 response body — happy path. Covers every contract-shaped
    /// field so the deserializer isn't silently dropping any.
    fn sample_artifact_body(id: &str) -> serde_json::Value {
        serde_json::json!({
            "artifact_id": id,
            "kind": "bundle",
            "manifest": { "name": "demo", "version": "1.0.0" },
            "content_hash": "deadbeef".repeat(8),
            "r2_url": "https://r2.example/bundle",
            "thumbnail_url": "https://r2.example/thumb.png",
            "author_pubkey": "aa".repeat(32),
            "author_fingerprint_hex": "aa11bb22cc33",
            "installs": 42,
            "reports": 1,
            "created_at": 1_700_000_000_i64,
            "updated_at": 1_700_001_000_i64,
            "status": "live",
        })
    }

    #[tokio::test]
    async fn get_artifact_returns_parsed_detail() {
        let server = MockServer::start().await;
        let body = sample_artifact_body("abc");
        Mock::given(method("GET"))
            .and(path("/v1/artifact/abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
            .mount(&server)
            .await;
        let client = test_client(&server.uri());
        let got = client.get_artifact("abc").await.expect("get_artifact ok");
        assert_eq!(got.artifact_id, "abc");
        assert_eq!(got.kind, "bundle");
        assert_eq!(got.status, "live");
        assert_eq!(got.installs, 42);
        assert_eq!(got.reports, 1);
        assert_eq!(got.created_at, 1_700_000_000);
        assert_eq!(got.updated_at, 1_700_001_000);
        assert_eq!(got.author_fingerprint_hex, "aa11bb22cc33");
        // `manifest` is a verbatim JSON subtree per worker-api §4.4.
        assert_eq!(got.manifest["name"], "demo");
    }

    #[tokio::test]
    async fn get_artifact_not_found_maps_to_server_reject() {
        let server = MockServer::start().await;
        let err_body = serde_json::json!({
            "error": { "code": "NOT_FOUND", "message": "no such artifact" }
        });
        Mock::given(method("GET"))
            .and(path("/v1/artifact/missing"))
            .respond_with(ResponseTemplate::new(404).set_body_json(err_body))
            .mount(&server)
            .await;
        let client = test_client(&server.uri());
        let err = client
            .get_artifact("missing")
            .await
            .expect_err("404 must fail");
        match err {
            UploadError::ServerReject {
                status, code, kind, ..
            } => {
                assert_eq!(status, 404);
                assert_eq!(code, "NOT_FOUND");
                // `default_kind_for_status` maps 404 → Malformed when no
                // explicit `kind` is in the body. Pins the current behavior.
                assert_eq!(kind, WorkerErrorKind::Malformed);
            }
            other => panic!("expected ServerReject, got {other:?}"),
        }
    }

    /// #015 T2 regression guard: when `ListParams::author_pubkey` is `Some`,
    /// the constructed `/v1/list` URL carries the `author_pubkey` query
    /// parameter. wiremock's `query_param` matcher only returns 200 when
    /// the query string contains the expected value — if a future refactor
    /// drops the field from `list()`'s query building, the mock falls
    /// through and the test fails.
    #[tokio::test]
    async fn list_appends_author_pubkey_query_param() {
        use wiremock::matchers::query_param;
        let server = MockServer::start().await;
        let expected_pk = "aa".repeat(32);
        Mock::given(method("GET"))
            .and(path("/v1/list"))
            .and(query_param("author_pubkey", expected_pk.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [],
                "next_cursor": null,
            })))
            .mount(&server)
            .await;
        let client = test_client(&server.uri());
        let lr = client
            .list(ListParams {
                kind: None,
                sort: None,
                tag: Vec::new(),
                cursor: None,
                limit: None,
                author_pubkey: Some(expected_pk.clone()),
            })
            .await
            .expect("list ok");
        assert!(lr.items.is_empty());
    }

    /// #015 T2 back-compat: when `author_pubkey` is `None`, the query
    /// string must NOT contain an `author_pubkey` param. Guards against
    /// accidentally serializing an empty string (which the worker's
    /// 64-hex regex would reject).
    #[tokio::test]
    async fn list_omits_author_pubkey_when_none() {
        use wiremock::matchers::query_param_is_missing;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/list"))
            .and(query_param_is_missing("author_pubkey"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "items": [],
                "next_cursor": null,
            })))
            .mount(&server)
            .await;
        let client = test_client(&server.uri());
        let lr = client
            .list(ListParams {
                kind: None,
                sort: None,
                tag: Vec::new(),
                cursor: None,
                limit: None,
                author_pubkey: None,
            })
            .await
            .expect("list ok");
        assert!(lr.items.is_empty());
    }

    /// Roundtrip-derive sanity: the struct is Serialize+Deserialize, so editors
    /// can re-emit what the host emits.
    #[test]
    fn artifact_detail_serde_roundtrip() {
        let body = sample_artifact_body("x1");
        let detail: ArtifactDetail = serde_json::from_value(body.clone()).unwrap();
        let back = serde_json::to_value(&detail).unwrap();
        assert_eq!(back["artifact_id"], body["artifact_id"]);
        assert_eq!(back["manifest"], body["manifest"]);
        assert_eq!(back["status"], body["status"]);
    }
}
