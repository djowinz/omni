//! Thin wrapper around `reqwest::Client` — the single seam where
//! `omni_identity::sign_http_jws` attaches `Authorization: Omni-JWS <compact>`.
//!
//! Do not strip this wrapper "for simplicity" in a later pass; scattering signing
//! across call-sites is the anti-pattern this file exists to prevent
//! (spec §4; retro-005 D3).

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use backon::ExponentialBuilder;
use base64::Engine;
use omni_guard_trait::Guard;
use omni_identity::{sign_http_jws, HttpJwsClaims, Keypair};
use reqwest::{Method, RequestBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;
use url::Url;

use super::cache::{ArtifactCache, ArtifactDetail};
use super::error::{UploadError, WorkerErrorKind};
use super::progress::UploadProgress;
use super::upload::{PackResult, UploadResult, UploadStatus};

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

#[derive(Debug, Clone)]
pub struct DownloadResult {
    pub bytes: Vec<u8>,
    pub content_hash: String,
    pub author_pubkey: String,
    pub signature: String,
    pub manifest_b64: String,
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

    fn pubkey_hex(&self) -> String {
        hex::encode(self.identity.public_key().0)
    }

    // ------------------------------------------------------------------
    // Public request methods (spec §4)
    // ------------------------------------------------------------------

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
                bytes: pack.sanitized_bytes.clone(),
            },
            MultipartPart {
                name: "thumbnail",
                filename: "thumbnail.png",
                content_type: "image/png",
                bytes: pack.thumbnail_png.clone(),
            },
        ];
        let (body_bytes, content_type) = serialize_multipart(&parts);

        let path = "/v1/upload";
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
            status: map_status(&body.status),
        };

        let pubkey_hex = self.pubkey_hex();
        let detail = ArtifactDetail {
            artifact_id: body.artifact_id.clone(),
            content_hash: body.content_hash,
            author_pubkey: pubkey_hex.clone(),
            name: pack.manifest_name,
            kind: pack.manifest_kind,
            r2_url: body.r2_url,
            thumbnail_url: body.thumbnail_url,
            updated_at: body.created_at,
        };
        self.cache
            .insert((pubkey_hex, body.artifact_id.clone()), detail)
            .await;

        Ok(result)
    }

    pub async fn download(&self, artifact_id: &str) -> Result<DownloadResult, UploadError> {
        let path = format!("/v1/download/{artifact_id}");
        let builder = self.sign(
            &Method::GET,
            &path,
            "",
            &[],
            self.http.get(self.url(&path)),
        )?;
        let resp = self.send_with_retry(builder).await?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        let headers = resp.headers().clone();
        let hget = |k: &str| {
            headers
                .get(k)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string()
        };
        let content_hash = hget("X-Omni-Content-Hash");
        let author_pubkey = hget("X-Omni-Author-Pubkey");
        let signature = hget("X-Omni-Signature");
        let manifest_b64 = hget("X-Omni-Manifest");
        let bytes = resp.bytes().await.map_err(UploadError::Network)?.to_vec();
        Ok(DownloadResult {
            bytes,
            content_hash,
            author_pubkey,
            signature,
            manifest_b64,
        })
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
        let builder = self
            .sign(&Method::GET, "/v1/list", &query, &[], self.http.get(url))?;
        let resp = self.send_with_retry(builder).await?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        let mut lr: ListResult = resp.json().await.map_err(UploadError::Network)?;
        let pubkey_hex = self.pubkey_hex();
        lr.items = self.cache.merge_into_list(&pubkey_hex, lr.items).await;
        Ok(lr)
    }

    pub async fn get(&self, id: &str) -> Result<ArtifactDetail, UploadError> {
        let path = format!("/v1/artifact/{id}");
        let builder = self.sign(
            &Method::GET,
            &path,
            "",
            &[],
            self.http.get(self.url(&path)),
        )?;
        let resp = self.send_with_retry(builder).await?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        resp.json::<ArtifactDetail>()
            .await
            .map_err(UploadError::Network)
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
        let (body_bytes, content_type) = serialize_multipart(&parts);
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
        let builder = self.sign(
            &Method::DELETE,
            &path,
            "",
            &[],
            self.http.delete(self.url(&path)),
        )?;
        let resp = self.send_with_retry(builder).await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(Self::decode_error(resp).await)
        }
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
        let builder = self.sign(&Method::POST, path, "", &raw, builder)?;
        let resp = self.send_with_retry(builder).await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(Self::decode_error(resp).await)
        }
    }

    pub async fn gallery(&self) -> Result<Vec<ArtifactDetail>, UploadError> {
        let path = "/v1/me/gallery";
        let builder =
            self.sign(&Method::GET, path, "", &[], self.http.get(self.url(path)))?;
        let resp = self.send_with_retry(builder).await?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        let lr: ListResult = resp.json().await.map_err(UploadError::Network)?;
        let pubkey_hex = self.pubkey_hex();
        Ok(self.cache.merge_into_list(&pubkey_hex, lr.items).await)
    }

    pub async fn config_vocab(&self) -> Result<VocabDoc, UploadError> {
        let path = "/v1/config/vocab";
        let builder =
            self.sign(&Method::GET, path, "", &[], self.http.get(self.url(path)))?;
        let resp = self.send_with_retry(builder).await?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        resp.json::<VocabDoc>().await.map_err(UploadError::Network)
    }

    pub async fn config_limits(&self) -> Result<omni_bundle::BundleLimits, UploadError> {
        let path = "/v1/config/limits";
        #[derive(Deserialize)]
        struct LimitsBody {
            max_bundle_compressed: u64,
            max_bundle_uncompressed: u64,
            max_entries: usize,
        }
        let builder =
            self.sign(&Method::GET, path, "", &[], self.http.get(self.url(path)))?;
        let resp = self.send_with_retry(builder).await?;
        if !resp.status().is_success() {
            return Err(Self::decode_error(resp).await);
        }
        let b: LimitsBody = resp.json().await.map_err(UploadError::Network)?;
        Ok(omni_bundle::BundleLimits {
            max_bundle_compressed: b.max_bundle_compressed,
            max_bundle_uncompressed: b.max_bundle_uncompressed,
            max_entries: b.max_entries,
        })
    }

    // ------------------------------------------------------------------
    // Internal: retry + error decode
    // ------------------------------------------------------------------

    async fn send_with_retry(
        &self,
        builder: RequestBuilder,
    ) -> Result<reqwest::Response, UploadError> {
        use backon::Retryable;
        let policy = self.retry_policy;
        // Capture a clonable builder for the closure; transient retries send a fresh clone.
        let base = builder.try_clone().ok_or_else(|| UploadError::BadInput {
            msg: "request not cloneable (streaming body)".into(),
            source: None,
        })?;
        let attempt = move || {
            let b = base
                .try_clone()
                .expect("builder cloneable (verified before retry loop)");
            async move { b.send().await }
        };
        attempt
            .retry(policy)
            .when(|e: &reqwest::Error| {
                e.is_connect() || e.is_timeout() || e.is_request()
            })
            .await
            .map_err(UploadError::Network)
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
                b.error.kind.unwrap_or_else(|| default_kind_for_status(status).into()),
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
        let kind = parse_kind(&kind_str);
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

fn map_status(s: &str) -> UploadStatus {
    match s {
        "deduplicated" => UploadStatus::Deduplicated,
        "updated" => UploadStatus::Updated,
        "unchanged" => UploadStatus::Unchanged,
        _ => UploadStatus::Created,
    }
}

fn parse_kind(s: &str) -> WorkerErrorKind {
    match s {
        "Malformed" => WorkerErrorKind::Malformed,
        "Unsafe" => WorkerErrorKind::Unsafe,
        "Integrity" => WorkerErrorKind::Integrity,
        "Auth" => WorkerErrorKind::Auth,
        "Quota" => WorkerErrorKind::Quota,
        "Admin" => WorkerErrorKind::Admin,
        _ => WorkerErrorKind::Io,
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
/// Worker verifies `body_sha256` over the exact transmitted bytes; building the
/// body ourselves guarantees we hash what we send. reqwest 0.12's
/// `multipart::Form` does not expose a buffered-bytes extractor, so per
/// writing-lessons rule #16 we implement the minimal subset of RFC 7578 we
/// need (fixed-count, filename-prefixed parts) rather than work around the
/// library. Boundary is a UUID-v4-style random string.
pub(crate) fn serialize_multipart(parts: &[MultipartPart]) -> (Vec<u8>, String) {
    // Boundary uniqueness requirement is "must not appear inside any part";
    // cryptographic randomness is not required. Use nanos-since-epoch + a
    // process-local monotonic counter — good enough and keeps the dep graph small.
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
        out.extend_from_slice(
            format!("Content-Type: {}\r\n\r\n", p.content_type).as_bytes(),
        );
        out.extend_from_slice(&p.bytes);
        out.extend_from_slice(b"\r\n");
    }
    out.extend_from_slice(b"--");
    out.extend_from_slice(boundary.as_bytes());
    out.extend_from_slice(b"--\r\n");

    let content_type = format!("multipart/form-data; boundary={boundary}");
    (out, content_type)
}
