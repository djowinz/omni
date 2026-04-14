use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::error::{BundleError, IntegrityKind, UnsafeKind};
use crate::hash::sha256_of;
use crate::manifest::{validate_manifest_references, Manifest};
use crate::path::{check_size, validate_path};
use crate::{
    MAX_BUNDLE_COMPRESSED, MAX_BUNDLE_UNCOMPRESSED, MAX_COMPRESSION_RATIO, MAX_ENTRIES,
};

pub fn unpack(
    zip_bytes: &[u8],
) -> Result<(Manifest, BTreeMap<String, Vec<u8>>), BundleError> {
    if (zip_bytes.len() as u64) > MAX_BUNDLE_COMPRESSED {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::SizeExceeded,
            detail: format!("bundle-compressed={} > {MAX_BUNDLE_COMPRESSED}", zip_bytes.len()),
        });
    }

    let mut zip = ZipArchive::new(Cursor::new(zip_bytes)).map_err(BundleError::from)?;

    if zip.len() > MAX_ENTRIES {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::TooManyEntries,
            detail: format!("{} entries", zip.len()),
        });
    }

    let mut declared_total: u64 = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(BundleError::from)?;
        if !entry.is_file() {
            return Err(BundleError::Unsafe {
                kind: UnsafeKind::Path,
                detail: format!("non-file entry: {}", entry.name()),
            });
        }
        if let Some(mode) = entry.unix_mode() {
            let ftype = mode & 0o170000;
            if ftype != 0 && ftype != 0o100000 {
                return Err(BundleError::Unsafe {
                    kind: UnsafeKind::Path,
                    detail: format!("non-regular entry: {} (mode {:o})", entry.name(), mode),
                });
            }
        }
        let compressed = entry.compressed_size();
        let uncompressed = entry.size();
        if compressed > 0 {
            // uncompressed > compressed * MAX_COMPRESSION_RATIO (no division; exact).
            let limit = compressed.saturating_mul(MAX_COMPRESSION_RATIO);
            if uncompressed > limit {
                let ratio = uncompressed / compressed.max(1);
                return Err(BundleError::Unsafe {
                    kind: UnsafeKind::ZipBomb,
                    detail: format!("ratio {ratio}:1"),
                });
            }
        }
        declared_total = declared_total.saturating_add(uncompressed);
    }
    if declared_total > MAX_BUNDLE_UNCOMPRESSED {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::SizeExceeded,
            detail: format!("bundle-uncompressed={declared_total} > {MAX_BUNDLE_UNCOMPRESSED}"),
        });
    }

    let manifest_bytes = {
        let mut m = zip.by_name("manifest.json").map_err(|_| BundleError::Integrity {
            kind: IntegrityKind::ManifestMissing,
            detail: String::new(),
        })?;
        let mut buf = Vec::with_capacity(m.size() as usize);
        m.read_to_end(&mut buf)?;
        buf
    };
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    validate_manifest_semantics(&manifest)?;

    let declared: BTreeMap<String, [u8; 32]> = manifest
        .files
        .iter()
        .map(|e| (e.path.clone(), e.sha256))
        .collect();

    let mut out: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(BundleError::from)?;
        let name = entry.name().to_string();
        if name == "manifest.json" {
            continue;
        }

        let kind = validate_path(&name)?;
        // Only check_size against the actual byte count; the declared
        // entry.size() is attacker-controlled central-directory data.
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        check_size(kind, bytes.len() as u64)?;

        let expected = declared
            .get(&name)
            .ok_or_else(|| BundleError::Integrity {
                kind: IntegrityKind::FileOrphan,
                detail: name.clone(),
            })?;
        let actual = sha256_of(&bytes);
        if &actual != expected {
            return Err(BundleError::Integrity {
                kind: IntegrityKind::HashMismatch,
                detail: format!(
                    "{name}: manifest={}, actual={}",
                    hex::encode(expected),
                    hex::encode(actual)
                ),
            });
        }

        out.insert(name, bytes);
    }

    for path in declared.keys() {
        if !out.contains_key(path) {
            return Err(BundleError::Integrity {
                kind: IntegrityKind::FileMissing,
                detail: path.clone(),
            });
        }
    }

    Ok((manifest, out))
}

fn validate_manifest_semantics(m: &Manifest) -> Result<(), BundleError> {
    if m.schema_version != 1 {
        return Err(BundleError::Integrity {
            kind: IntegrityKind::SchemaVersionUnsupported,
            detail: format!("schema_version {} not supported", m.schema_version),
        });
    }
    if !m.entry_overlay.ends_with(".omni") {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::Path,
            detail: format!("entry_overlay not .omni: {}", m.entry_overlay),
        });
    }
    for e in &m.files {
        let _ = validate_path(&e.path)?;
    }
    validate_manifest_references(m)
}
