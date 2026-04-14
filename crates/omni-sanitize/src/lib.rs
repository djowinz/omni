//! Omni theme / bundle sanitization pipeline.
//!
//! Consumes (Manifest, files) produced by omni_identity::unpack_signed_bundle,
//! dispatches each file to a per-kind handler via manifest.resource_kinds
//! (retro-005 D5 / invariant #5), runs the executable-magic deny-list
//! (retro-005 D11 / invariant #19c), and returns sanitized file contents
//! plus a SanitizeReport. Re-packing and signing are done upstream by
//! omni_identity::pack_signed_bundle.
//!
//! WASM-clean: no std::fs, no threading, no IO.

mod error;
mod handlers;
mod magic;

pub use error::{
    FileKind, FileReport, SanitizeError, SanitizeReport, SanitizeVersion, SANITIZE_VERSION,
};

use std::collections::BTreeMap;

use omni_bundle::Manifest;

/// Sanitize a single standalone CSS theme. Task 9 wires the body.
pub fn sanitize_theme(_css_bytes: &[u8]) -> Result<(Vec<u8>, SanitizeReport), SanitizeError> {
    todo!("wired in Task 9")
}

/// Sanitize an already-verified bundle. Task 9 wires the body.
pub fn sanitize_bundle(
    _manifest: &Manifest,
    _files: BTreeMap<String, Vec<u8>>,
) -> Result<(BTreeMap<String, Vec<u8>>, SanitizeReport), SanitizeError> {
    todo!("wired in Task 9")
}
