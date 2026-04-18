//! Omni theme / bundle sanitization pipeline.
//!
//! Consumes (Manifest, files) produced by identity::unpack_signed_bundle,
//! dispatches each file to a per-kind handler via manifest.resource_kinds
//! (retro-005 D5 / invariant #5), runs the executable-magic deny-list
//! (retro-005 D11 / invariant #19c), and returns sanitized file contents
//! plus a SanitizeReport. Re-packing and signing are done upstream by
//! identity::pack_signed_bundle.
//!
//! WASM-clean: no std::fs, no threading, no IO.

mod error;
mod handlers;
mod magic;

#[cfg(feature = "wasm")]
pub mod wasm;

pub use error::{
    FileKind, FileReport, SanitizeError, SanitizeReport, SanitizeVersion, SANITIZE_VERSION,
};

use std::collections::BTreeMap;

use bundle::Manifest;
use sha2::{Digest, Sha256};

use crate::handlers::{dispatch_for_path, supported_kind_names, Handler, ThemeHandler, HANDLERS};
use crate::magic::reject_executable_magic;

/// Sanitize a single standalone CSS theme. Runs the executable-magic deny-list
/// (invariant #19c) before invoking the theme handler.
pub fn sanitize_theme(css_bytes: &[u8]) -> Result<(Vec<u8>, SanitizeReport), SanitizeError> {
    if let Err(sig) = reject_executable_magic(css_bytes) {
        return Err(SanitizeError::RejectedExecutableMagic {
            prefix_hex: hex::encode(sig),
            path: "theme.css".into(),
        });
    }
    let original_sha256 = sha256(css_bytes);
    let out = ThemeHandler.sanitize("theme.css", css_bytes)?;
    let sanitized_sha256 = sha256(&out);
    let report = SanitizeReport {
        version: SANITIZE_VERSION,
        original_size: css_bytes.len() as u64,
        sanitized_size: out.len() as u64,
        files: vec![FileReport {
            path: "theme.css".into(),
            kind: FileKind::Theme,
            original_sha256,
            sanitized_sha256,
        }],
    };
    Ok((out, report))
}

/// Sanitize an already-verified bundle.
pub fn sanitize_bundle(
    manifest: &Manifest,
    files: BTreeMap<String, Vec<u8>>,
) -> Result<(BTreeMap<String, Vec<u8>>, SanitizeReport), SanitizeError> {
    if manifest.schema_version != 1 {
        return Err(SanitizeError::Malformed {
            message: format!("unsupported schema_version {}", manifest.schema_version),
            source: Some(Box::new(bundle::BundleError::Integrity {
                kind: bundle::IntegrityKind::SchemaVersionUnsupported,
                detail: format!("schema_version={}", manifest.schema_version),
            })),
        });
    }

    if let Some(rk) = manifest.resource_kinds.as_ref() {
        for kind_name in rk.keys() {
            if !HANDLERS.iter().any(|h| h.kind() == kind_name.as_str()) {
                return Err(SanitizeError::UnknownResourceKind {
                    kind: kind_name.clone(),
                    supported: supported_kind_names(),
                });
            }
        }
    }

    let original_size: u64 = files.values().map(|v| v.len() as u64).sum();
    let mut out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut reports: Vec<FileReport> = Vec::new();

    for (path, bytes) in files {
        if let Err(sig) = reject_executable_magic(&bytes) {
            return Err(SanitizeError::RejectedExecutableMagic {
                prefix_hex: hex::encode(sig),
                path,
            });
        }
        let (handler, max_size) = dispatch_for_path(&path, manifest.resource_kinds.as_ref())?;
        if (bytes.len() as u64) > max_size {
            return Err(SanitizeError::SizeExceeded {
                path,
                actual: bytes.len() as u64,
                limit: max_size,
            });
        }
        let original_sha256 = sha256(&bytes);
        let sanitized = handler.sanitize(&path, &bytes)?;
        let sanitized_sha256 = sha256(&sanitized);
        reports.push(FileReport {
            path: path.clone(),
            kind: handler.file_kind(),
            original_sha256,
            sanitized_sha256,
        });
        out.insert(path, sanitized);
    }

    let sanitized_size: u64 = out.values().map(|v| v.len() as u64).sum();
    let report = SanitizeReport {
        version: SANITIZE_VERSION,
        original_size,
        sanitized_size,
        files: reports,
    };
    Ok((out, report))
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}
