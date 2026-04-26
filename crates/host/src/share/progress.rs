//! `UploadProgress` — orchestration-internal enum + WS adapter.
//!
//! Contract (ws-explorer.md §`upload.publish`) emits `{ phase, done, total }` where
//! `phase ∈ {"pack", "sanitize", "upload"}`. Internal variants preserve higher-fidelity
//! logs while the adapter collapses to the contract shape.

use serde::Serialize;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::error::UploadError;
use super::upload::UploadResult;

#[derive(Debug, Clone)]
pub enum UploadProgress {
    Packing,
    Sanitizing { file: String },
    Signing,
    Uploading { sent: u64, total: u64 },
    Done { result: UploadResult },
}

/// Contract-shape frame per ws-explorer.md.
#[derive(Debug, Clone, Serialize)]
pub struct WireProgress {
    pub phase: &'static str, // "pack" | "sanitize" | "upload"
    pub done: u64,
    pub total: u64,
}

impl UploadProgress {
    /// Map internal variant to the contract's `{ phase, done, total }` frame.
    /// Returns `None` for `Done` (terminal events use `*Result` / `error` envelopes).
    pub fn to_wire(&self) -> Option<WireProgress> {
        match self {
            Self::Packing => Some(WireProgress {
                phase: "pack",
                done: 0,
                total: 0,
            }),
            Self::Sanitizing { .. } => Some(WireProgress {
                phase: "sanitize",
                done: 0,
                total: 0,
            }),
            Self::Signing => Some(WireProgress {
                phase: "upload",
                done: 0,
                total: 0,
            }),
            Self::Uploading { sent, total } => Some(WireProgress {
                phase: "upload",
                done: *sent,
                total: *total,
            }),
            Self::Done { .. } => None,
        }
    }
}

/// Forward every event from `rx` as a `upload.publishProgress` frame (or terminal
/// `upload.publishResult`) onto `send_fn` keyed by the editor's request `id`.
///
/// `send_fn` is the existing file_api/ws_server text broadcaster — `Fn(String) + Send`.
/// Returns the final `UploadResult` on success or `UploadError::Cancelled` if the
/// sender is dropped before a `Done` frame arrives. Upload failures surface through
/// the `upload()` return value, not through this pump.
pub async fn pump_to_ws<F>(
    request_id: &str,
    result_type: &str, // "upload.publishResult" | "upload.updateResult"
    mut rx: mpsc::Receiver<UploadProgress>,
    send_fn: F,
) -> Result<UploadResult, UploadError>
where
    F: Fn(String) + Send,
{
    while let Some(ev) = rx.recv().await {
        if let Some(wire) = UploadProgress::to_wire(&ev) {
            let frame = json!({
                "id": request_id,
                "type": "upload.publishProgress",
                "params": wire,
            });
            send_fn(frame.to_string());
        }
        if let UploadProgress::Done { result } = ev {
            let frame = json!({
                "id": request_id,
                "type": result_type,
                "params": {
                    "artifact_id": result.artifact_id,
                    "content_hash": result.content_hash,
                    "status": result.status.as_str(),
                    "worker_url": result.r2_url,
                },
            });
            send_fn(frame.to_string());
            return Ok(result);
        }
    }
    Err(UploadError::Cancelled)
}

/// Build the `{ code, kind, detail, message }` error envelope for a given `UploadError`.
///
/// `UploadError::DependencyViolations` carries an extra `violations` array on
/// the inner `error` object (OWI-40 / Task A1.6). The renderer's
/// `PackingViolationsCard` (INV-7.8.5) reads it directly without re-parsing
/// the textual `detail` field. All other variants emit the canonical
/// `{ code, kind, detail, message }` shape.
pub fn error_envelope(request_id: &str, err: &UploadError) -> Value {
    let (kind, detail) = match err {
        UploadError::ServerReject { kind, detail, .. } => (kind.to_string(), detail.clone()),
        UploadError::Io(_) => ("Io".into(), None),
        UploadError::BadInput { msg, .. } => ("Malformed".into(), Some(msg.clone())),
        UploadError::Network(_) => ("Io".into(), None),
        UploadError::Integrity { msg, .. } => ("Integrity".into(), Some(msg.clone())),
        UploadError::DependencyViolations { .. } => ("Malformed".into(), None),
        UploadError::Cancelled => ("Io".into(), None),
    };
    let mut error_obj = json!({
        "code": err.code(),
        "kind": kind,
        "detail": detail,
        "message": err.user_message(),
    });
    if let UploadError::DependencyViolations { violations } = err {
        // Serialize via DependencyViolationDetail's serde impl so the wire
        // shape matches the renderer's PackingViolation struct (kind/path/detail).
        if let Some(map) = error_obj.as_object_mut() {
            map.insert(
                "violations".into(),
                serde_json::to_value(violations).unwrap_or(Value::Null),
            );
        }
    }
    json!({
        "id": request_id,
        "type": "error",
        "error": error_obj,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_variants_map_to_contract_phases() {
        assert_eq!(UploadProgress::Packing.to_wire().unwrap().phase, "pack");
        assert_eq!(
            UploadProgress::Sanitizing { file: "x".into() }
                .to_wire()
                .unwrap()
                .phase,
            "sanitize"
        );
        assert_eq!(UploadProgress::Signing.to_wire().unwrap().phase, "upload");
        let up = UploadProgress::Uploading {
            sent: 10,
            total: 100,
        }
        .to_wire()
        .unwrap();
        assert_eq!(up.phase, "upload");
        assert_eq!(up.done, 10);
        assert_eq!(up.total, 100);
    }

    #[test]
    fn done_variant_has_no_wire_frame() {
        assert!(UploadProgress::Done {
            result: UploadResult {
                artifact_id: "a".into(),
                content_hash: "h".into(),
                r2_url: "".into(),
                thumbnail_url: "".into(),
                status: super::super::upload::UploadStatus::Created,
            }
        }
        .to_wire()
        .is_none());
    }
}
