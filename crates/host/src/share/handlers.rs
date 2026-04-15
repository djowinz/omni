//! WebSocket handler layer for `explorer.*` messages.
//!
//! Spec: `2026-04-10-theme-sharing-010-host-install-pipeline.md` §7 + §9.
//!
//! This module currently provides the D-004-J error-envelope mapping
//! (`map_install_error`) that any future dispatcher wiring will use. The
//! dispatcher itself is deferred — the existing `ws_server::handle_message`
//! path is synchronous (no tokio runtime, no access to `ShareClient` /
//! `PreviewSlot`), and wiring it up requires extending `WsSharedState` plus
//! threading an async executor through the WS thread. That refactor is
//! tracked as follow-up.
//!
//! The pieces below are the portable, testable part: the error vocabulary
//! the editor binds to. No `#[source]` chain from third-party crates
//! (`reqwest`, `zip`, `serde_json`, `ots`) ever reaches the wire — invariant
//! #19a.

use serde::{Deserialize, Serialize};

use crate::share::install::{BadBundleKind, InstallError};

/// D-004-J error payload shape. Serializes into the `error` field of an
/// `explorer.*` error envelope:
///
/// ```json
/// { "id": "...", "type": "error",
///   "error": { "code": "...", "kind": "...", "detail": "...", "message": "..." } }
/// ```
///
/// - `code` is the stable machine-readable identifier the editor switches on.
/// - `kind` is the broader category (`Io` / `Malformed` / `Unsafe` /
///   `Integrity` / `HostLocal`) for UX grouping.
/// - `detail` is an opaque sub-kind string for structured host-side logging;
///   the editor treats it as opaque.
/// - `message` is a short human-readable string suitable for toast display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorPayload {
    pub code: &'static str,
    pub kind: &'static str,
    pub detail: String,
    pub message: &'static str,
}

/// Map an [`InstallError`] onto the wire-safe [`ErrorPayload`] envelope.
///
/// Per invariant #19a the `#[source]` chain is NOT surfaced — third-party
/// error text (`reqwest::Error`, `zip::Error`, `serde_json::Error`, etc.)
/// stays in host logs, never on the wire.
pub fn map_install_error(err: &InstallError) -> ErrorPayload {
    match err {
        InstallError::IoFailure(e) => ErrorPayload {
            code: "io_error",
            kind: "Io",
            detail: format!("io:{}", e.kind()),
            message: "Failed to write bundle to disk.",
        },
        InstallError::BadBundle { kind, .. } => match kind {
            BadBundleKind::Malformed => ErrorPayload {
                code: "bundle_malformed",
                kind: "Malformed",
                detail: "bundle:malformed".into(),
                message: "Bundle structure is invalid.",
            },
            BadBundleKind::Unsafe => ErrorPayload {
                code: "bundle_unsafe",
                kind: "Unsafe",
                detail: "bundle:unsafe".into(),
                message: "Bundle contained unsafe content.",
            },
            BadBundleKind::Integrity => ErrorPayload {
                code: "bundle_integrity_failure",
                kind: "Integrity",
                detail: "bundle:integrity".into(),
                message: "Bundle failed integrity checks.",
            },
        },
        InstallError::SignatureFailed(_) => ErrorPayload {
            code: "signature_invalid",
            kind: "Integrity",
            detail: "signature:invalid".into(),
            message: "Bundle signature could not be verified.",
        },
        InstallError::TofuViolation { .. } => ErrorPayload {
            code: "tofu_mismatch",
            kind: "Integrity",
            detail: "tofu:display_name_mismatch".into(),
            message: "This author's fingerprint has changed since last install.",
        },
        InstallError::VersionMismatch { .. } => ErrorPayload {
            code: "version_mismatch",
            kind: "Malformed",
            detail: "version:mismatch".into(),
            message: "Bundle requires a newer Omni version.",
        },
        InstallError::Cancelled => ErrorPayload {
            code: "cancelled",
            kind: "HostLocal",
            detail: "cancelled:user".into(),
            message: "Install was cancelled.",
        },
    }
}

/// Envelope returned by `explorer.*` dispatcher arms when the host's
/// `InstallContext` is not yet constructed (pre-async-bridge chore).
///
/// Mirrors sub-spec #009's `ShareContext`-None pattern: dispatcher arms
/// always emit this envelope today; once the async-bridge chore wires
/// `InstallContext` onto `WsSharedState` each arm will branch on
/// `state.install_context().is_some()` and only fall through to this
/// helper when the context really is missing.
pub fn install_context_unavailable() -> ErrorPayload {
    ErrorPayload {
        code: "service_unavailable",
        kind: "HostLocal",
        detail: "install_context_not_constructed".into(),
        message: "Install service is not available yet.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::io;

    /// Build one sample value per `InstallError` variant so the mapper sees
    /// each arm. Anytime a new variant is added, this list must grow — the
    /// `distinct_codes` test below will otherwise fail on cardinality.
    fn one_of_each_variant() -> Vec<InstallError> {
        vec![
            InstallError::IoFailure(io::Error::other("disk full")),
            InstallError::BadBundle {
                kind: BadBundleKind::Malformed,
                detail: "x".into(),
                source: None,
            },
            InstallError::BadBundle {
                kind: BadBundleKind::Unsafe,
                detail: "x".into(),
                source: None,
            },
            InstallError::BadBundle {
                kind: BadBundleKind::Integrity,
                detail: "x".into(),
                source: None,
            },
            InstallError::SignatureFailed("bad jws".into()),
            InstallError::TofuViolation {
                known: "aa".into(),
                seen: "bb".into(),
            },
            InstallError::VersionMismatch {
                required: semver::Version::new(2, 0, 0),
                current: semver::Version::new(1, 0, 0),
            },
            InstallError::Cancelled,
        ]
    }

    #[test]
    fn every_variant_has_a_distinct_code() {
        let samples = one_of_each_variant();
        let expected = samples.len();
        let codes: HashSet<&'static str> =
            samples.iter().map(|e| map_install_error(e).code).collect();
        assert_eq!(
            codes.len(),
            expected,
            "every InstallError variant must map to a unique code; got {codes:?}"
        );
    }

    #[test]
    fn install_context_unavailable_envelope_is_pinned() {
        // This test pins the wire-format constants the editor binds to.
        // If either `code` or `kind` changes, every editor consumer must
        // update in lockstep — failing this test forces a coordinated
        // update.
        let payload = install_context_unavailable();
        assert_eq!(payload.code, "service_unavailable");
        assert_eq!(payload.kind, "HostLocal");
        assert_eq!(payload.detail, "install_context_not_constructed");
    }

    #[test]
    fn source_chain_never_appears_in_payload() {
        // Simulate a reqwest-ish error riding in the #[source] chain: if the
        // mapper ever interpolated `source` into `detail` or `message`, the
        // marker string below would leak to the wire.
        let reqwest_like: Box<dyn std::error::Error + Send + Sync> =
            Box::new(io::Error::other("reqwest::Error: connection reset"));
        let err = InstallError::BadBundle {
            kind: BadBundleKind::Malformed,
            detail: "structural".into(),
            source: Some(reqwest_like),
        };
        let payload = map_install_error(&err);
        assert!(
            !payload.detail.contains("reqwest"),
            "detail leaked source chain: {}",
            payload.detail
        );
        assert!(
            !payload.message.contains("reqwest"),
            "message leaked source chain: {}",
            payload.message
        );
        // Also guard against the raw `detail` string from the variant leaking;
        // the mapper deliberately emits opaque sub-kinds, not user-supplied
        // strings.
        assert!(
            !payload.detail.contains("structural"),
            "detail leaked variant-provided string: {}",
            payload.detail
        );
    }
}
