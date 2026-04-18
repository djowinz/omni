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
use serde_json::json;

use crate::share::install::{BadBundleKind, InstallError, InstallOutcome, InstallProgress};
use crate::share::preview::PreviewError;
use identity::TofuResult;

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

/// Envelope returned by `explorer.*` dispatcher arms when `share_ctx` is
/// `None` or the async-bridge chore hasn't wired install handlers yet.
///
/// Dispatcher arms always emit this envelope today; once the async-bridge
/// chore wires explorer.* handlers onto `ShareContext`, each arm will
/// branch on `share_ctx.is_some()` and only fall through to this helper
/// when the context really is missing.
pub fn install_context_unavailable() -> ErrorPayload {
    ErrorPayload {
        code: "service_unavailable",
        kind: "HostLocal",
        detail: "install_context_not_constructed".into(),
        message: "Install service is not available yet.",
    }
}

/// Serialize an [`InstallProgress`] step into a wire-ready
/// `explorer.installProgress` frame. Added for #021 so the async handler
/// layer can stream progress to the editor without inlining JSON
/// construction at every call site.
///
/// Contract shape (per `specs/contracts/ws-explorer.md` §explorer.install):
/// `{ id, type: "explorer.installProgress", phase, done, total }` where
/// `phase ∈ {download, verify, sanitize, write}`. The shipped
/// `InstallProgress` variants collapse into these four phases:
/// - `Downloading { received, total }` → `download`
/// - `Verifying` → `verify` (done=0, total=0)
/// - `Sanitizing` → `sanitize` (done=0, total=0)
/// - `Writing { index, total, .. }` → `write` (done=index, total=total)
/// - `Committing` → `write` (done=total=1 signals final write step)
pub fn install_progress_to_contract_frame(id: &str, progress: InstallProgress) -> String {
    let (phase, done, total) = match progress {
        InstallProgress::Downloading { received, total } => ("download", received, total),
        InstallProgress::Verifying => ("verify", 0, 0),
        InstallProgress::Sanitizing => ("sanitize", 0, 0),
        InstallProgress::Writing { index, total, .. } => ("write", index as u64, total as u64),
        InstallProgress::Committing => ("write", 1, 1),
    };
    json!({
        "id": id,
        "type": "explorer.installProgress",
        "phase": phase,
        "done": done,
        "total": total,
    })
    .to_string()
}

/// Map an [`InstallOutcome`] into a wire-ready `explorer.installResult`
/// frame. Added for #021. Contract shape (per
/// `specs/contracts/ws-explorer.md` §explorer.install):
/// `{ id, type: "explorer.installResult", installed_path, content_hash,
///    author_fingerprint_hex, tofu, warnings }`.
///
/// `content_hash` is hex-encoded (32-byte SHA-256). `author_fingerprint_hex`
/// is the six-byte fingerprint, hex-encoded, per invariant #20. `tofu` is
/// the stable string vocab `{first_install, matched, mismatch}` — not the
/// raw Rust enum debug form. `warnings` is a flat array of short strings
/// derived from `InstallWarning` variants.
pub fn install_outcome_to_result_frame(id: &str, outcome: &InstallOutcome) -> String {
    let tofu = tofu_result_to_wire_str(&outcome.tofu);
    let warnings: Vec<String> = outcome
        .warnings
        .iter()
        .map(|w| match w {
            crate::share::install::InstallWarning::ExceedsCurrentPolicy {
                kind,
                actual,
                limit,
            } => format!("exceeds_policy:{kind}:actual={actual}:limit={limit}"),
        })
        .collect();
    json!({
        "id": id,
        "type": "explorer.installResult",
        "installed_path": outcome.installed_path.to_string_lossy(),
        "content_hash": hex::encode(outcome.content_hash),
        "author_fingerprint_hex": outcome.fingerprint.to_hex(),
        "tofu": tofu,
        "warnings": warnings,
    })
    .to_string()
}

fn tofu_result_to_wire_str(r: &TofuResult) -> &'static str {
    match r {
        TofuResult::FirstSeen => "first_install",
        TofuResult::KnownMatch => "matched",
        TofuResult::DisplayNameMismatch { .. } => "mismatch",
    }
}

/// Map a [`PreviewError`] into the wire-safe [`ErrorPayload`] envelope.
/// Added for #021 so `handle_preview` / `handle_cancel_preview` can emit
/// the D-004-J error shape without inlining per-variant matches.
pub fn map_preview_error(e: &PreviewError) -> ErrorPayload {
    match e {
        PreviewError::PreviewActive => ErrorPayload {
            code: "PREVIEW_ACTIVE",
            kind: "HostLocal",
            detail: "preview:slot_occupied".into(),
            message: "A preview is already active; cancel it first.",
        },
        PreviewError::NoActivePreview => ErrorPayload {
            code: "NO_ACTIVE_PREVIEW",
            kind: "HostLocal",
            detail: "preview:no_session_for_token".into(),
            message: "No active preview matches that token.",
        },
        PreviewError::TokenMismatch => ErrorPayload {
            code: "TOKEN_MISMATCH",
            kind: "HostLocal",
            detail: "preview:token_mismatch".into(),
            message: "The supplied preview token does not match the active session.",
        },
        PreviewError::ApplyFailed(_) => ErrorPayload {
            code: "PREVIEW_APPLY_FAILED",
            kind: "HostLocal",
            detail: "preview:apply_failed".into(),
            message: "Failed to apply the preview theme.",
        },
    }
}

/// Wrap an [`ErrorPayload`] in the standard D-004-J error envelope. Added
/// for #021 as the canonical error-frame builder used by every `explorer.*`
/// handler path (async-dispatch arms in `ws_messages.rs` plus the
/// `share_ctx`-absent fallback in `ws_server.rs`). Mirrors the envelope
/// shape emitted inline in the pre-#021 explorer.* stub so wire behavior
/// is unchanged.
///
/// Envelope shape: `{ id, type: "error", error: { code, kind, detail, message } }`.
pub fn error_frame(id: &str, payload: &ErrorPayload) -> String {
    json!({
        "id": id,
        "type": "error",
        "error": {
            "code": payload.code,
            "kind": payload.kind,
            "detail": payload.detail,
            "message": payload.message,
        },
    })
    .to_string()
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

    // ---- #021 contract-serializer tests --------------------------------

    use crate::share::install::{InstallOutcome, InstallProgress, InstallWarning};
    use identity::{Keypair, PublicKey, TofuResult};
    use serde_json::Value as JsonValue;

    fn all_progress_variants() -> Vec<InstallProgress> {
        vec![
            InstallProgress::Downloading {
                received: 100,
                total: 200,
            },
            InstallProgress::Verifying,
            InstallProgress::Sanitizing,
            InstallProgress::Writing {
                file: "overlay.omni".into(),
                index: 3,
                total: 7,
            },
            InstallProgress::Committing,
        ]
    }

    #[test]
    fn install_progress_to_contract_frame_for_each_variant() {
        let id = "req-1";
        for variant in all_progress_variants() {
            let frame = install_progress_to_contract_frame(id, variant.clone());
            let parsed: JsonValue = serde_json::from_str(&frame).expect("valid json");
            assert_eq!(parsed["id"], id, "id missing/wrong for {variant:?}");
            assert_eq!(
                parsed["type"], "explorer.installProgress",
                "type missing/wrong for {variant:?}"
            );
            let phase = parsed["phase"].as_str().expect("phase is a string");
            assert!(
                matches!(phase, "download" | "verify" | "sanitize" | "write"),
                "phase {phase:?} not in allowed set for {variant:?}"
            );
            assert!(
                parsed["done"].is_number(),
                "done must be a number for {variant:?}"
            );
            assert!(
                parsed["total"].is_number(),
                "total must be a number for {variant:?}"
            );
        }
    }

    #[test]
    fn install_progress_to_contract_frame_maps_downloading() {
        let frame = install_progress_to_contract_frame(
            "r",
            InstallProgress::Downloading {
                received: 42,
                total: 100,
            },
        );
        let parsed: JsonValue = serde_json::from_str(&frame).unwrap();
        assert_eq!(parsed["phase"], "download");
        assert_eq!(parsed["done"], 42);
        assert_eq!(parsed["total"], 100);
    }

    #[test]
    fn install_progress_to_contract_frame_maps_writing_index_and_total() {
        let frame = install_progress_to_contract_frame(
            "r",
            InstallProgress::Writing {
                file: "a.css".into(),
                index: 2,
                total: 5,
            },
        );
        let parsed: JsonValue = serde_json::from_str(&frame).unwrap();
        assert_eq!(parsed["phase"], "write");
        assert_eq!(parsed["done"], 2);
        assert_eq!(parsed["total"], 5);
    }

    #[test]
    fn install_outcome_to_result_frame_shape() {
        let kp = Keypair::generate();
        let pk: PublicKey = kp.public_key();
        let fp = pk.fingerprint();
        let outcome = InstallOutcome {
            installed_path: std::path::PathBuf::from("/tmp/omni/bundles/x"),
            content_hash: [0xAB; 32],
            author_pubkey: pk,
            fingerprint: fp,
            tofu: TofuResult::FirstSeen,
            warnings: vec![InstallWarning::ExceedsCurrentPolicy {
                kind: "entry_size".into(),
                actual: 1_000_000,
                limit: 500_000,
            }],
        };
        let frame = install_outcome_to_result_frame("req-9", &outcome);
        let parsed: JsonValue = serde_json::from_str(&frame).unwrap();
        assert_eq!(parsed["id"], "req-9");
        assert_eq!(parsed["type"], "explorer.installResult");
        assert_eq!(
            parsed["installed_path"].as_str().unwrap(),
            outcome.installed_path.to_string_lossy()
        );
        assert_eq!(
            parsed["content_hash"].as_str().unwrap(),
            hex::encode([0xAB_u8; 32])
        );
        assert_eq!(parsed["author_fingerprint_hex"], fp.to_hex());
        assert_eq!(parsed["tofu"], "first_install");
        let warnings = parsed["warnings"].as_array().expect("warnings array");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0]
            .as_str()
            .unwrap()
            .contains("exceeds_policy:entry_size"));
    }

    #[test]
    fn install_outcome_to_result_frame_maps_tofu_variants() {
        let kp = Keypair::generate();
        let pk: PublicKey = kp.public_key();
        let fp = pk.fingerprint();
        let base = |tofu| InstallOutcome {
            installed_path: std::path::PathBuf::from("/tmp/x"),
            content_hash: [0; 32],
            author_pubkey: pk,
            fingerprint: fp,
            tofu,
            warnings: vec![],
        };
        let f1 = install_outcome_to_result_frame("r", &base(TofuResult::FirstSeen));
        let f2 = install_outcome_to_result_frame("r", &base(TofuResult::KnownMatch));
        let f3 = install_outcome_to_result_frame(
            "r",
            &base(TofuResult::DisplayNameMismatch {
                known_pubkey_hex: "aa".into(),
                seen_pubkey_hex: "bb".into(),
                display_name: "x".into(),
            }),
        );
        let p1: JsonValue = serde_json::from_str(&f1).unwrap();
        let p2: JsonValue = serde_json::from_str(&f2).unwrap();
        let p3: JsonValue = serde_json::from_str(&f3).unwrap();
        assert_eq!(p1["tofu"], "first_install");
        assert_eq!(p2["tofu"], "matched");
        assert_eq!(p3["tofu"], "mismatch");
    }

    fn all_preview_error_variants() -> Vec<PreviewError> {
        vec![
            PreviewError::PreviewActive,
            PreviewError::NoActivePreview,
            PreviewError::TokenMismatch,
            PreviewError::ApplyFailed("boom".into()),
        ]
    }

    #[test]
    fn map_preview_error_each_variant() {
        let variants = all_preview_error_variants();
        let codes: HashSet<&'static str> =
            variants.iter().map(|e| map_preview_error(e).code).collect();
        assert_eq!(
            codes.len(),
            variants.len(),
            "every PreviewError variant must map to a unique code; got {codes:?}"
        );
        for v in &variants {
            let payload = map_preview_error(v);
            assert_eq!(
                payload.kind, "HostLocal",
                "variant {v:?} should be HostLocal"
            );
            assert!(!payload.code.is_empty(), "variant {v:?} has empty code");
            assert!(
                !payload.message.is_empty(),
                "variant {v:?} has empty message"
            );
        }
    }

    #[test]
    fn map_preview_error_pins_wire_codes() {
        // Pins the stable-string vocab the editor binds to. Any code rename
        // must land with a coordinated editor update.
        assert_eq!(
            map_preview_error(&PreviewError::PreviewActive).code,
            "PREVIEW_ACTIVE"
        );
        assert_eq!(
            map_preview_error(&PreviewError::NoActivePreview).code,
            "NO_ACTIVE_PREVIEW"
        );
        assert_eq!(
            map_preview_error(&PreviewError::TokenMismatch).code,
            "TOKEN_MISMATCH"
        );
        assert_eq!(
            map_preview_error(&PreviewError::ApplyFailed("x".into())).code,
            "PREVIEW_APPLY_FAILED"
        );
    }

    #[test]
    fn error_frame_matches_pre_021_envelope_shape() {
        let payload = install_context_unavailable();
        let frame = error_frame("req-42", &payload);
        let parsed: JsonValue = serde_json::from_str(&frame).unwrap();
        assert_eq!(parsed["id"], "req-42");
        assert_eq!(parsed["type"], "error");
        assert_eq!(parsed["error"]["code"], "service_unavailable");
        assert_eq!(parsed["error"]["kind"], "HostLocal");
        assert_eq!(parsed["error"]["detail"], "install_context_not_constructed");
        assert_eq!(
            parsed["error"]["message"],
            "Install service is not available yet."
        );
    }
}
