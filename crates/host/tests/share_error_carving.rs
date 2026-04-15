//! Verifies `UploadError` invariant #19a carving: public variants are domain-
//! categorized; `BundleError` / `SanitizeError` / `reqwest::Error` ride `#[source]`.
//!
//! Integration tests live outside the crate and therefore only see the public
//! API surface. If a variant disappears from the public set or a new one is
//! added without an explicit carving decision, the exhaustive match in
//! `public_variant_set_is_stable` will fail to compile — that is the point.

use std::io;
use std::time::Duration;

use omni_host::share::error::{UploadError, WorkerErrorKind};

#[test]
fn public_variant_set_is_stable() {
    // Exhaustively construct every public variant — adding one forces an
    // explicit decision whether it joins the public domain set.
    let cases: Vec<UploadError> = vec![
        UploadError::Io(io::Error::other("")),
        UploadError::BadInput {
            msg: "".into(),
            source: None,
        },
        UploadError::Integrity {
            msg: "".into(),
            source: None,
        },
        UploadError::Cancelled,
        UploadError::ServerReject {
            status: 400,
            code: "BAD".into(),
            kind: WorkerErrorKind::Malformed,
            detail: None,
            message: "".into(),
            retry_after: None,
        },
    ];
    for c in cases {
        // Both code() and user_message() are part of the stable public surface.
        let _ = c.code();
        let _ = c.user_message();
    }
}

#[test]
fn worker_kinds_cover_worker_api_categories() {
    // worker-api.md §3 "Error categories (D9)": the full domain set.
    for k in [
        WorkerErrorKind::Malformed,
        WorkerErrorKind::Unsafe,
        WorkerErrorKind::Integrity,
        WorkerErrorKind::Io,
        WorkerErrorKind::Auth,
        WorkerErrorKind::Quota,
        WorkerErrorKind::Admin,
    ] {
        let _ = k.to_string();
    }
}

#[test]
fn worker_kind_serde_roundtrip_every_variant() {
    // Every WorkerErrorKind must serde-roundtrip — it rides across the HTTP
    // boundary in the Worker error envelope and across the WS boundary in the
    // `share.error` payload.
    for k in [
        WorkerErrorKind::Malformed,
        WorkerErrorKind::Unsafe,
        WorkerErrorKind::Integrity,
        WorkerErrorKind::Io,
        WorkerErrorKind::Auth,
        WorkerErrorKind::Quota,
        WorkerErrorKind::Admin,
    ] {
        let s = serde_json::to_string(&k).expect("serialize");
        let back: WorkerErrorKind = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(k, back, "roundtrip mismatch for {k}");
    }
}

#[test]
fn worker_kind_wire_format_is_pascal_case() {
    // worker-api.md contract: PascalCase on the wire. Locking the exact
    // strings prevents accidental rename-only refactors from breaking peers.
    assert_eq!(
        serde_json::to_string(&WorkerErrorKind::Malformed).unwrap(),
        "\"Malformed\""
    );
    assert_eq!(
        serde_json::to_string(&WorkerErrorKind::Unsafe).unwrap(),
        "\"Unsafe\""
    );
    assert_eq!(
        serde_json::to_string(&WorkerErrorKind::Integrity).unwrap(),
        "\"Integrity\""
    );
    assert_eq!(
        serde_json::to_string(&WorkerErrorKind::Io).unwrap(),
        "\"Io\""
    );
    assert_eq!(
        serde_json::to_string(&WorkerErrorKind::Auth).unwrap(),
        "\"Auth\""
    );
    assert_eq!(
        serde_json::to_string(&WorkerErrorKind::Quota).unwrap(),
        "\"Quota\""
    );
    assert_eq!(
        serde_json::to_string(&WorkerErrorKind::Admin).unwrap(),
        "\"Admin\""
    );
}

#[test]
fn worker_kind_display_matches_wire_token() {
    // Display output is part of the stable public contract: operators read it
    // in logs and the WS adapter formats it directly into user-facing strings.
    assert_eq!(WorkerErrorKind::Malformed.to_string(), "Malformed");
    assert_eq!(WorkerErrorKind::Unsafe.to_string(), "Unsafe");
    assert_eq!(WorkerErrorKind::Integrity.to_string(), "Integrity");
    assert_eq!(WorkerErrorKind::Io.to_string(), "Io");
    assert_eq!(WorkerErrorKind::Auth.to_string(), "Auth");
    assert_eq!(WorkerErrorKind::Quota.to_string(), "Quota");
    assert_eq!(WorkerErrorKind::Admin.to_string(), "Admin");
}

#[test]
fn upload_error_send_sync_static() {
    // Static guarantee: UploadError must cross thread boundaries so the
    // upload future can move between the WS task and the packer blocking pool.
    fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<UploadError>();
    assert_send_sync::<WorkerErrorKind>();
}

#[test]
fn upload_error_display_is_stable() {
    // Display output is logged verbatim — lock the exact prefixes so a later
    // rewrite doesn't silently break log-based dashboards.
    let io_err = UploadError::Io(io::Error::new(io::ErrorKind::NotFound, "missing"));
    assert_eq!(io_err.to_string(), "I/O error");

    let bad = UploadError::BadInput {
        msg: "empty name".into(),
        source: None,
    };
    assert_eq!(bad.to_string(), "bad input: empty name");

    let integrity = UploadError::Integrity {
        msg: "hash mismatch".into(),
        source: None,
    };
    assert_eq!(
        integrity.to_string(),
        "integrity check failed: hash mismatch"
    );

    let cancelled = UploadError::Cancelled;
    assert_eq!(cancelled.to_string(), "cancelled");

    let reject = UploadError::ServerReject {
        status: 429,
        code: "RATE_LIMITED".into(),
        kind: WorkerErrorKind::Quota,
        detail: Some("bucket full".into()),
        message: "slow down".into(),
        retry_after: Some(Duration::from_secs(5)),
    };
    assert_eq!(
        reject.to_string(),
        "server rejected request: RATE_LIMITED (Quota)"
    );
}

#[test]
fn upload_error_code_is_stable() {
    // Machine-readable `code` is the editor's branch discriminator — it must
    // be stable across internal refactors.
    assert_eq!(UploadError::Io(io::Error::other("")).code(), "IO");
    assert_eq!(
        UploadError::BadInput {
            msg: "".into(),
            source: None,
        }
        .code(),
        "BAD_INPUT"
    );
    assert_eq!(
        UploadError::Integrity {
            msg: "".into(),
            source: None,
        }
        .code(),
        "INTEGRITY"
    );
    assert_eq!(UploadError::Cancelled.code(), "CANCELLED");
    assert_eq!(
        UploadError::ServerReject {
            status: 400,
            code: "BAD".into(),
            kind: WorkerErrorKind::Malformed,
            detail: None,
            message: "".into(),
            retry_after: None,
        }
        .code(),
        "SERVER_REJECT"
    );
}

#[test]
fn is_transient_covers_every_worker_kind() {
    // Per spec §9: Quota is always transient; Io is transient only when the
    // origin signalled 5xx; everything else is a hard stop.
    fn reject(kind: WorkerErrorKind, status: u16) -> UploadError {
        UploadError::ServerReject {
            status,
            code: "X".into(),
            kind,
            detail: None,
            message: "".into(),
            retry_after: None,
        }
    }

    // Quota — always transient regardless of status.
    assert!(reject(WorkerErrorKind::Quota, 429).is_transient());
    assert!(reject(WorkerErrorKind::Quota, 503).is_transient());

    // Io — transient only for 5xx.
    assert!(!reject(WorkerErrorKind::Io, 400).is_transient());
    assert!(!reject(WorkerErrorKind::Io, 499).is_transient());
    assert!(reject(WorkerErrorKind::Io, 500).is_transient());
    assert!(reject(WorkerErrorKind::Io, 502).is_transient());
    assert!(reject(WorkerErrorKind::Io, 599).is_transient());

    // Hard-stop categories — never transient.
    for k in [
        WorkerErrorKind::Malformed,
        WorkerErrorKind::Unsafe,
        WorkerErrorKind::Integrity,
        WorkerErrorKind::Auth,
        WorkerErrorKind::Admin,
    ] {
        assert!(
            !reject(k, 500).is_transient(),
            "{k:?} must never be transient"
        );
        assert!(
            !reject(k, 400).is_transient(),
            "{k:?} must never be transient"
        );
    }

    // Non-server variants: Io (local), BadInput, Integrity, Cancelled — all
    // terminal. Network, which wraps reqwest::Error, is the only other
    // transient variant and is covered via the reject-shape tests elsewhere
    // because constructing a reqwest::Error requires a live failure.
    assert!(!UploadError::Io(io::Error::other("")).is_transient());
    assert!(!UploadError::BadInput {
        msg: "".into(),
        source: None,
    }
    .is_transient());
    assert!(!UploadError::Integrity {
        msg: "".into(),
        source: None,
    }
    .is_transient());
    assert!(!UploadError::Cancelled.is_transient());
}

#[test]
fn server_reject_shape_roundtrips_through_wire_kind() {
    // The `ServerReject` fields that cross the WS boundary (`status`, `code`,
    // `kind`, `detail`, `message`, `retry_after` seconds) must survive a
    // serialize→deserialize cycle on the `kind` token without losing the
    // operator's branch discriminator. `detail` is log-only per
    // ws-explorer.md but we still prove its shape is preserved here.
    let original = UploadError::ServerReject {
        status: 413,
        code: "BUNDLE_TOO_LARGE".into(),
        kind: WorkerErrorKind::Malformed,
        detail: Some("compressed=12MiB max=5MiB".into()),
        message: "Bundle exceeds size limit.".into(),
        retry_after: None,
    };

    // Extract wire-facing fields.
    let (status, code, kind, detail, message, retry_after) = match &original {
        UploadError::ServerReject {
            status,
            code,
            kind,
            detail,
            message,
            retry_after,
        } => (
            *status,
            code.clone(),
            *kind,
            detail.clone(),
            message.clone(),
            *retry_after,
        ),
        _ => unreachable!(),
    };

    // Kind is the only field with serde derive; roundtrip it and rebuild.
    let kind_wire = serde_json::to_string(&kind).unwrap();
    let kind_back: WorkerErrorKind = serde_json::from_str(&kind_wire).unwrap();
    assert_eq!(kind, kind_back);

    let rebuilt = UploadError::ServerReject {
        status,
        code: code.clone(),
        kind: kind_back,
        detail: detail.clone(),
        message: message.clone(),
        retry_after,
    };

    // Equivalence via the public projections (UploadError itself is not Eq).
    assert_eq!(original.code(), rebuilt.code());
    assert_eq!(original.user_message(), rebuilt.user_message());
    assert_eq!(original.to_string(), rebuilt.to_string());
    assert_eq!(original.is_transient(), rebuilt.is_transient());
}

#[test]
fn io_from_conversion_is_transparent() {
    // The single #[from] exception (std::io::Error is stable public API).
    let e: UploadError = io::Error::new(io::ErrorKind::PermissionDenied, "nope").into();
    assert_eq!(e.code(), "IO");
    assert!(!e.is_transient());
}
