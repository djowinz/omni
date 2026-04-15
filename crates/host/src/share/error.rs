//! Domain-carved upload errors (architectural invariant #19a).
//!
//! Variants are carved by what the editor/host must distinguish, NOT by producing crate.
//! `io::Error` is the one `#[from]` exception (stable std public API).
//! `BundleError`, `SanitizeError`, `IdentityError`, `GuardError`, and `reqwest::Error`
//! ride in `#[source]` chains so this enum stays stable across their version bumps.

use std::io;
use std::time::Duration;

use serde::{Deserialize, Serialize};

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
