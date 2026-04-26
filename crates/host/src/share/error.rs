//! Domain-carved upload errors (architectural invariant #19a).
//!
//! Variants are carved by what the editor/host must distinguish, NOT by producing crate.
//! `io::Error` is the one `#[from]` exception (stable std public API).
//! `BundleError`, `SanitizeError`, `IdentityError`, `GuardError`, and `reqwest::Error`
//! ride in `#[source]` chains so this enum stays stable across their version bumps.

use std::io;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// One row of the structured dependency-violation payload (OWI-40 / Task A1.6).
///
/// Mirrors the renderer's `PackingViolation` shape (see
/// `apps/desktop/renderer/components/omni/upload-dialog/steps/packing-violations-card.tsx`)
/// so the Step 3 aggregate-violations card (INV-7.8.5) can render the host's
/// `upload.packResult` failure envelope without translation.
///
/// Fields:
/// * `kind` — closed vocabulary `"missing-ref" | "unused-file" |
///   "content-safety"`. Stays a `String` so the third kind (introduced in
///   Wave B1.5 / OWI-54 alongside the ONNX moderator) rides the same wire
///   shape without a contract churn.
/// * `path` — workspace-relative path of the offending file.
/// * `detail` — optional human-readable reason; the renderer falls back to
///   a kind-default when absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyViolationDetail {
    pub kind: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
}

/// Worker error-kind domain. Mirrors `worker-api.md` §3 "Error categories (D9)".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum WorkerErrorKind {
    Malformed,
    Unsafe,
    Integrity,
    Io,
    Auth,
    Quota,
    Admin,
}

impl std::fmt::Display for WorkerErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Malformed => "Malformed",
            Self::Unsafe => "Unsafe",
            Self::Integrity => "Integrity",
            Self::Io => "Io",
            Self::Auth => "Auth",
            Self::Quota => "Quota",
            Self::Admin => "Admin",
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UploadError {
    #[error("I/O error")]
    Io(#[from] io::Error),

    #[error("bad input: {msg}")]
    BadInput {
        msg: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("network error")]
    Network(#[source] reqwest::Error),

    #[error("server rejected request: {code} ({kind})")]
    ServerReject {
        status: u16,
        code: String,
        kind: WorkerErrorKind,
        detail: Option<String>,
        message: String,
        retry_after: Option<Duration>,
    },

    #[error("integrity check failed: {msg}")]
    Integrity {
        msg: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Dependency Check stage failure (OWI-40 / Task A1.6 + Wave B1.5).
    /// Carries the aggregated `Vec<DependencyViolationDetail>` directly so the
    /// `error_envelope` can serialize them as a structured `violations` array
    /// the renderer's `PackingViolationsCard` (INV-7.8.5) reads without
    /// re-parsing a string. Per spec INV-7.3.7 / INV-7.8.4, the resolver
    /// accumulates ALL violations across categories before failing — this
    /// variant carries the full list, never just the first.
    #[error("dependency check failed: {} violations", violations.len())]
    DependencyViolations {
        violations: Vec<DependencyViolationDetail>,
    },

    #[error("cancelled")]
    Cancelled,
}

impl UploadError {
    /// Short machine-readable code for the `{ code, kind, detail, message }` WS payload.
    /// Editors MUST branch on this; `detail` is log-only per ws-explorer.md §"Error payload shape".
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "IO",
            Self::BadInput { .. } => "BAD_INPUT",
            Self::Network(_) => "NETWORK",
            Self::ServerReject { .. } => "SERVER_REJECT",
            Self::Integrity { .. } => "INTEGRITY",
            Self::DependencyViolations { .. } => "DEPENDENCY_VIOLATIONS",
            Self::Cancelled => "CANCELLED",
        }
    }

    /// User-facing message rendered by the editor (see ws-explorer.md "D-004-J").
    pub fn user_message(&self) -> String {
        match self {
            Self::Io(e) => format!("File I/O failed: {e}"),
            Self::BadInput { msg, .. } => format!("Invalid upload: {msg}"),
            Self::Network(_) => {
                "Network error contacting the theme service. Check your connection.".into()
            }
            Self::ServerReject { message, .. } => message.clone(),
            Self::Integrity { msg, .. } => format!("Integrity check failed: {msg}"),
            Self::DependencyViolations { violations } => format!(
                "{} dependency violation{}",
                violations.len(),
                if violations.len() == 1 { "" } else { "s" }
            ),
            Self::Cancelled => "Upload cancelled.".into(),
        }
    }

    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network(_) => true,
            Self::ServerReject {
                kind: WorkerErrorKind::Quota,
                ..
            } => true,
            Self::ServerReject {
                kind: WorkerErrorKind::Io,
                status,
                ..
            } if *status >= 500 => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_sync_static() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UploadError>();
        assert_send_sync::<WorkerErrorKind>();
    }

    #[test]
    fn kind_serde_roundtrip() {
        for k in [
            WorkerErrorKind::Malformed,
            WorkerErrorKind::Unsafe,
            WorkerErrorKind::Integrity,
            WorkerErrorKind::Io,
            WorkerErrorKind::Auth,
            WorkerErrorKind::Quota,
            WorkerErrorKind::Admin,
        ] {
            let s = serde_json::to_string(&k).unwrap();
            let back: WorkerErrorKind = serde_json::from_str(&s).unwrap();
            assert_eq!(k, back);
        }
    }

    #[test]
    fn io_from_is_transparent() {
        let e: UploadError = io::Error::new(io::ErrorKind::NotFound, "x").into();
        assert_eq!(e.code(), "IO");
    }

    #[test]
    fn is_transient_matches_spec() {
        let q = UploadError::ServerReject {
            status: 429,
            code: "RATE_LIMITED".into(),
            kind: WorkerErrorKind::Quota,
            detail: None,
            message: "slow down".into(),
            retry_after: Some(Duration::from_secs(30)),
        };
        assert!(q.is_transient());

        let auth = UploadError::ServerReject {
            status: 401,
            code: "AUTH_BAD_SIGNATURE".into(),
            kind: WorkerErrorKind::Auth,
            detail: None,
            message: "nope".into(),
            retry_after: None,
        };
        assert!(!auth.is_transient());
    }
}
