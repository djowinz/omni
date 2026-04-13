use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use sha2::{Digest, Sha256};
use zip::ZipArchive;

use crate::error::BundleError;
use crate::manifest::Manifest;
use crate::path::{check_size, validate_path};
use crate::{
    MAX_BUNDLE_COMPRESSED, MAX_BUNDLE_UNCOMPRESSED, MAX_COMPRESSION_RATIO, MAX_ENTRIES,
};

pub fn unpack(
    zip_bytes: &[u8],
) -> Result<(Manifest, BTreeMap<String, Vec<u8>>), BundleError> {
    if (zip_bytes.len() as u64) > MAX_BUNDLE_COMPRESSED {
        return Err(BundleError::SizeExceeded {
            kind: "bundle-compressed".into(),
            actual: zip_bytes.len() as u64,
            limit: MAX_BUNDLE_COMPRESSED,
        });
    }

    let mut zip = ZipArchive::new(Cursor::new(zip_bytes)).map_err(BundleError::Zip)?;

    if zip.len() > MAX_ENTRIES {
        return Err(BundleError::TooManyEntries { actual: zip.len() });
    }

    let mut declared_total: u64 = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index(i).map_err(BundleError::Zip)?;
        if !entry.is_file() {
            return Err(BundleError::UnsafePath(format!(
                "non-file entry: {}",
                entry.name()
            )));
        }
        if let Some(mode) = entry.unix_mode() {
            let ftype = mode & 0o170000;
            if ftype != 0 && ftype != 0o100000 {
                return Err(BundleError::UnsafePath(format!(
                    "non-regular entry: {} (mode {:o})",
                    entry.name(),
                    mode
                )));
            }
        }
        let compressed = entry.compressed_size();
        let uncompressed = entry.size();
        if compressed > 0 {
            // uncompressed > compressed * MAX_COMPRESSION_RATIO (no division; exact).
            let limit = compressed.saturating_mul(MAX_COMPRESSION_RATIO);
            if uncompressed > limit {
                let ratio = uncompressed / compressed.max(1);
                return Err(BundleError::ZipBomb(ratio));
            }
        }
        declared_total = declared_total.saturating_add(uncompressed);
    }
    if declared_total > MAX_BUNDLE_UNCOMPRESSED {
        return Err(BundleError::SizeExceeded {
            kind: "bundle-uncompressed".into(),
            actual: declared_total,
            limit: MAX_BUNDLE_UNCOMPRESSED,
        });
    }

    let manifest_bytes = {
        let mut m = zip.by_name("manifest.json").map_err(|_| BundleError::ManifestMissing)?;
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
        let mut entry = zip.by_index(i).map_err(BundleError::Zip)?;
        let name = entry.name().to_string();
        if name == "manifest.json" {
            continue;
        }

        let kind = validate_path(&name)?;
        check_size(kind, entry.size())?;

        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        check_size(kind, bytes.len() as u64)?;

        let expected = declared
            .get(&name)
            .ok_or_else(|| BundleError::FileOrphan(name.clone()))?;
        let actual = sha256_of(&bytes);
        if &actual != expected {
            return Err(BundleError::HashMismatch {
                path: name.clone(),
                manifest: hex::encode(expected),
                actual: hex::encode(actual),
            });
        }

        out.insert(name, bytes);
    }

    for path in declared.keys() {
        if !out.contains_key(path) {
            return Err(BundleError::FileMissing(path.clone()));
        }
    }

    Ok((manifest, out))
}

fn validate_manifest_semantics(m: &Manifest) -> Result<(), BundleError> {
    if m.schema_version != 1 {
        return Err(BundleError::InvalidVersion(format!(
            "schema_version {} not supported",
            m.schema_version
        )));
    }
    if !m.entry_overlay.ends_with(".omni") {
        return Err(BundleError::UnsafePath(format!(
            "entry_overlay not .omni: {}",
            m.entry_overlay
        )));
    }
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for e in &m.files {
        let _ = validate_path(&e.path)?;
        if !seen.insert(e.path.as_str()) {
            return Err(BundleError::UnsafePath(format!("duplicate manifest entry: {}", e.path)));
        }
    }
    if !m.files.iter().any(|e| e.path == m.entry_overlay) {
        return Err(BundleError::FileMissing(m.entry_overlay.clone()));
    }
    if let Some(theme) = &m.default_theme {
        if !m.files.iter().any(|e| &e.path == theme) {
            return Err(BundleError::FileMissing(theme.clone()));
        }
    }
    Ok(())
}

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut d = [0u8; 32];
    d.copy_from_slice(&out);
    d
}
