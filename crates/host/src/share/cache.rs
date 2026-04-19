//! Post-upload artifact cache (60 s TTL, 128 entries) — D1/KV eventual-consistency workaround.
//!
//! Per spec §7: after a successful upload/patch, insert the returned metadata;
//! subsequent `list`/`gallery`/`get` responses merge with live cache before returning.

use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use serde::{Deserialize, Serialize};

/// Post-upload cache shape: the subset of artifact metadata the cache stores
/// to paper over KV eventual-consistency between `POST /v1/upload` and the
/// following `list`/`gallery` read.
///
/// Note: this is distinct from [`super::client::ArtifactDetail`], which
/// mirrors the full `GET /v1/artifact/:id` wire shape (worker-api §4.4) —
/// `manifest`, `reports`, `status`, etc. that the cache has no use for.
/// Renamed from `ArtifactDetail` in the phase-2 follow-up that added
/// `ShareClient::get_artifact`, to free the `ArtifactDetail` name for the
/// contract-matching struct in `client.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedArtifactDetail {
    pub artifact_id: String,
    pub content_hash: String,
    pub author_pubkey: String,
    pub name: String,
    pub kind: String, // "theme" | "bundle"
    // `/v1/list` rows don't include `r2_url` — that URL is only emitted by
    // `/v1/artifact/:id` (see ArtifactDetail in client.rs). Marking this
    // `#[serde(default)]` lets the same struct deserialize both shapes:
    // cached-from-detail rows keep the real URL, list-derived rows get "".
    // Consumers that need the URL must fetch `/v1/artifact/:id` via
    // `ShareClient::get_artifact` or read it via the cache's post-upload
    // merge path (which always has the full detail).
    #[serde(default)]
    pub r2_url: String,
    pub thumbnail_url: String,
    pub updated_at: i64,
}

/// `(pubkey_hex, artifact_id)` is the cache key per spec §7.
pub type CacheKey = (String, String);

#[derive(Clone)]
pub struct ArtifactCache {
    inner: Arc<Cache<CacheKey, CachedArtifactDetail>>,
}

impl ArtifactCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(
                Cache::builder()
                    .max_capacity(128)
                    .time_to_live(Duration::from_secs(60))
                    .build(),
            ),
        }
    }

    pub async fn insert(&self, key: CacheKey, value: CachedArtifactDetail) {
        self.inner.insert(key, value).await;
    }

    #[cfg(test)]
    pub(crate) async fn get(&self, key: &CacheKey) -> Option<CachedArtifactDetail> {
        self.inner.get(key).await
    }

    /// Merge cache entries for `author_pubkey` into the server-returned list.
    /// Cached entries override server entries with the same `artifact_id`; cached
    /// entries not in the server list are prepended (most-recently-uploaded first).
    pub async fn merge_into_list(
        &self,
        author_pubkey: &str,
        server_items: Vec<CachedArtifactDetail>,
    ) -> Vec<CachedArtifactDetail> {
        let mut out: Vec<CachedArtifactDetail> = Vec::with_capacity(server_items.len() + 4);
        let mut seen = std::collections::HashSet::new();

        // Walk cache first so fresh uploads appear at top.
        for (key, value) in self.inner.iter() {
            if key.0 == author_pubkey {
                seen.insert(key.1.clone());
                out.push(value);
            }
        }
        for item in server_items {
            if !seen.contains(&item.artifact_id) {
                out.push(item);
            }
        }
        out
    }
}

impl Default for ArtifactCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> CachedArtifactDetail {
        CachedArtifactDetail {
            artifact_id: id.into(),
            content_hash: "h".into(),
            author_pubkey: "pk".into(),
            name: id.into(),
            kind: "theme".into(),
            r2_url: "".into(),
            thumbnail_url: "".into(),
            updated_at: 0,
        }
    }

    #[tokio::test]
    async fn insert_get_roundtrip() {
        let c = ArtifactCache::new();
        c.insert(("pk".into(), "a".into()), sample("a")).await;
        let got = c.get(&("pk".into(), "a".into())).await.unwrap();
        assert_eq!(got.artifact_id, "a");
    }

    #[tokio::test]
    async fn merge_prepends_fresh_and_dedups() {
        let c = ArtifactCache::new();
        c.insert(("pk".into(), "fresh".into()), sample("fresh"))
            .await;
        let server = vec![sample("old"), sample("fresh")];
        let merged = c.merge_into_list("pk", server).await;
        // cached "fresh" comes first; server "fresh" deduped; "old" preserved
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].artifact_id, "fresh");
        assert_eq!(merged[1].artifact_id, "old");
    }

    #[tokio::test]
    async fn other_author_not_merged() {
        let c = ArtifactCache::new();
        c.insert(("other_pk".into(), "x".into()), sample("x")).await;
        let server = vec![sample("a")];
        let merged = c.merge_into_list("pk", server).await;
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].artifact_id, "a");
    }
}
