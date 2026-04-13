use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use sha2::{Digest, Sha256};
use zip::write::FileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::error::BundleError;
use crate::manifest::{pretty_manifest_bytes, Manifest};
use crate::path::{check_size, validate_path, FileKind};
use crate::{MAX_BUNDLE_COMPRESSED, MAX_BUNDLE_UNCOMPRESSED, MAX_ENTRIES};

/// Pack a manifest + file map into a deterministic `.omnipkg` zip.
pub fn pack(
    manifest: &Manifest,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, BundleError> {
    if files.contains_key("manifest.json") {
        return Err(BundleError::FileOrphan(
            "manifest.json must not appear in files map; it is computed".into(),
        ));
    }

    let entries: BTreeMap<&str, &[u8; 32]> = manifest
        .files
        .iter()
        .map(|e| (e.path.as_str(), &e.sha256))
        .collect();

    for entry_path in entries.keys() {
        if !files.contains_key(*entry_path) {
            return Err(BundleError::FileMissing((*entry_path).to_string()));
        }
    }
    for fkey in files.keys() {
        if !entries.contains_key(fkey.as_str()) {
            return Err(BundleError::FileOrphan(fkey.clone()));
        }
    }

    let mut total_uncompressed: u64 = 0;
    for (path, bytes) in files.iter() {
        let kind = validate_path(path)?;
        check_size(kind, bytes.len() as u64)?;
        total_uncompressed = total_uncompressed.saturating_add(bytes.len() as u64);

        let expected = entries[path.as_str()];
        let actual = sha256_of(bytes);
        if actual != *expected {
            return Err(BundleError::HashMismatch {
                path: path.clone(),
                manifest: hex::encode(expected),
                actual: hex::encode(actual),
            });
        }
    }

    let total_entries = files.len() + 1;
    if total_entries > MAX_ENTRIES {
        return Err(BundleError::TooManyEntries { actual: total_entries });
    }

    let manifest_bytes = pretty_manifest_bytes(manifest).map_err(BundleError::Json)?;
    check_size(FileKind::Manifest, manifest_bytes.len() as u64)?;
    total_uncompressed = total_uncompressed.saturating_add(manifest_bytes.len() as u64);
    if total_uncompressed > MAX_BUNDLE_UNCOMPRESSED {
        return Err(BundleError::SizeExceeded {
            kind: "bundle-uncompressed".into(),
            actual: total_uncompressed,
            limit: MAX_BUNDLE_UNCOMPRESSED,
        });
    }

    let buf = Vec::<u8>::with_capacity(total_uncompressed as usize);
    let mut zw = ZipWriter::new(Cursor::new(buf));

    let options = FileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default())
        .unix_permissions(0o644);

    zw.start_file("manifest.json", options).map_err(BundleError::Zip)?;
    zw.write_all(&manifest_bytes)?;

    for (path, bytes) in files.iter() {
        zw.start_file(path.as_str(), options).map_err(BundleError::Zip)?;
        zw.write_all(bytes)?;
    }

    let cursor = zw.finish().map_err(BundleError::Zip)?;
    let out = cursor.into_inner();

    if (out.len() as u64) > MAX_BUNDLE_COMPRESSED {
        return Err(BundleError::SizeExceeded {
            kind: "bundle-compressed".into(),
            actual: out.len() as u64,
            limit: MAX_BUNDLE_COMPRESSED,
        });
    }

    Ok(out)
}

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut d = [0u8; 32];
    d.copy_from_slice(&out);
    d
}
