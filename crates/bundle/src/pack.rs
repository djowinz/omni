use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use zip::write::FileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::error::{BundleError, IntegrityKind, UnsafeKind};
use crate::hash::sha256_of;
use crate::manifest::{pretty_manifest_bytes, validate_manifest_references, Manifest};
use crate::path::validate_path;
use crate::BundleLimits;

/// Pack a manifest + file map into a deterministic `.omnipkg` zip.
pub fn pack(
    manifest: &Manifest,
    files: &BTreeMap<String, Vec<u8>>,
    limits: &BundleLimits,
) -> Result<Vec<u8>, BundleError> {
    if files.contains_key("manifest.json") {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::Path,
            detail: "manifest.json is a reserved name; remove it from files map".into(),
        });
    }

    // Fast-fail structural checks before any hashing.
    let total_entries = files.len() + 1;
    if total_entries > limits.max_entries {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::TooManyEntries,
            detail: format!("{total_entries} entries"),
        });
    }

    validate_manifest_references(manifest)?;

    let entries: BTreeMap<&str, &[u8; 32]> = manifest
        .files
        .iter()
        .map(|e| (e.path.as_str(), &e.sha256))
        .collect();

    for entry_path in entries.keys() {
        if !files.contains_key(*entry_path) {
            return Err(BundleError::Integrity {
                kind: IntegrityKind::FileMissing,
                detail: (*entry_path).to_string(),
            });
        }
    }
    for fkey in files.keys() {
        if !entries.contains_key(fkey.as_str()) {
            return Err(BundleError::Integrity {
                kind: IntegrityKind::FileOrphan,
                detail: fkey.clone(),
            });
        }
    }

    let mut total_uncompressed: u64 = 0;
    for (path, bytes) in files.iter() {
        validate_path(path)?;
        total_uncompressed = total_uncompressed.saturating_add(bytes.len() as u64);

        let expected = entries[path.as_str()];
        let actual = sha256_of(bytes);
        if actual != *expected {
            return Err(BundleError::Integrity {
                kind: IntegrityKind::HashMismatch,
                detail: format!(
                    "{path}: manifest={}, actual={}",
                    hex::encode(expected),
                    hex::encode(actual)
                ),
            });
        }
    }

    let manifest_bytes = pretty_manifest_bytes(manifest).map_err(BundleError::from)?;
    total_uncompressed = total_uncompressed.saturating_add(manifest_bytes.len() as u64);
    if total_uncompressed > limits.max_bundle_uncompressed {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::SizeExceeded,
            detail: format!(
                "bundle-uncompressed={total_uncompressed} > {}",
                limits.max_bundle_uncompressed
            ),
        });
    }

    let buf = Vec::<u8>::with_capacity(total_uncompressed as usize);
    let mut zw = ZipWriter::new(Cursor::new(buf));

    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);

    zw.start_file("manifest.json", options)
        .map_err(BundleError::from)?;
    zw.write_all(&manifest_bytes)?;

    for (path, bytes) in files.iter() {
        zw.start_file(path.as_str(), options)
            .map_err(BundleError::from)?;
        zw.write_all(bytes)?;
    }

    let cursor = zw.finish().map_err(BundleError::from)?;
    let out = cursor.into_inner();

    if (out.len() as u64) > limits.max_bundle_compressed {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::SizeExceeded,
            detail: format!(
                "bundle-compressed={} > {}",
                out.len(),
                limits.max_bundle_compressed
            ),
        });
    }

    Ok(out)
}
