use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::error::{BundleError, IntegrityKind, UnsafeKind};
use crate::hash::sha256_of;
use crate::manifest::{validate_manifest_references, Manifest};
use crate::path::validate_path;
use crate::BundleLimits;
use crate::MAX_COMPRESSION_RATIO;

/// Fast path: parse and validate the manifest only. Does NOT decompress
/// file contents. Use this for callers that only need metadata (duplicate-
/// upload checks, TOFU lookup, explorer-preview metadata).
pub fn unpack_manifest(
    zip_bytes: &[u8],
    limits: &BundleLimits,
) -> Result<Manifest, BundleError> {
    if (zip_bytes.len() as u64) > limits.max_bundle_compressed {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::SizeExceeded,
            detail: format!(
                "bundle-compressed={} > {}",
                zip_bytes.len(),
                limits.max_bundle_compressed
            ),
        });
    }

    let mut zip = ZipArchive::new(Cursor::new(zip_bytes))?;
    if zip.len() > limits.max_entries {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::TooManyEntries,
            detail: format!("{} > {}", zip.len(), limits.max_entries),
        });
    }

    let manifest_bytes = {
        let mut m = zip
            .by_name("manifest.json")
            .map_err(|e| match e {
                zip::result::ZipError::FileNotFound => BundleError::Integrity {
                    kind: IntegrityKind::ManifestMissing,
                    detail: String::new(),
                },
                other => BundleError::from(other),
            })?;
        let mut buf = Vec::with_capacity(m.size() as usize);
        m.read_to_end(&mut buf)?;
        buf
    };
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    validate_manifest_semantics(&manifest)?;
    Ok(manifest)
}

/// Full unpack: returns an `Unpack` handle that holds the parsed archive
/// and manifest. Files are streamed via `files()` — peak memory is one
/// file at a time, not the full bundle.
pub fn unpack<'a>(
    zip_bytes: &'a [u8],
    limits: &BundleLimits,
) -> Result<Unpack<'a>, BundleError> {
    if (zip_bytes.len() as u64) > limits.max_bundle_compressed {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::SizeExceeded,
            detail: format!(
                "bundle-compressed={} > {}",
                zip_bytes.len(),
                limits.max_bundle_compressed
            ),
        });
    }

    let mut zip = ZipArchive::new(Cursor::new(zip_bytes))?;
    if zip.len() > limits.max_entries {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::TooManyEntries,
            detail: format!("{} > {}", zip.len(), limits.max_entries),
        });
    }

    // Pre-flight: structure, ratio, declared totals.
    let mut declared_total: u64 = 0;
    for i in 0..zip.len() {
        let entry = zip.by_index(i)?;
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
    if declared_total > limits.max_bundle_uncompressed {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::SizeExceeded,
            detail: format!(
                "bundle-uncompressed={} > {}",
                declared_total, limits.max_bundle_uncompressed
            ),
        });
    }

    // Manifest first.
    let manifest_bytes = {
        let mut m = zip
            .by_name("manifest.json")
            .map_err(|e| match e {
                zip::result::ZipError::FileNotFound => BundleError::Integrity {
                    kind: IntegrityKind::ManifestMissing,
                    detail: String::new(),
                },
                other => BundleError::from(other),
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

    Ok(Unpack {
        zip,
        manifest,
        declared,
        next_index: 0,
        seen: BTreeMap::new(),
    })
}

pub struct Unpack<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    manifest: Manifest,
    declared: BTreeMap<String, [u8; 32]>,
    next_index: usize,
    /// Track which declared files have been yielded for missing-file check.
    seen: BTreeMap<String, ()>,
}

impl std::fmt::Debug for Unpack<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Unpack")
            .field("manifest", &self.manifest)
            .field("next_index", &self.next_index)
            .finish_non_exhaustive()
    }
}

pub struct UnpackedFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

impl<'a> Unpack<'a> {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Streaming iterator over all non-manifest files. Each yielded file is
    /// validated against its declared SHA-256. When the iterator is
    /// exhausted, callers should call `finalize()` to verify no declared
    /// files are missing.
    pub fn files(&mut self) -> Files<'_, 'a> {
        Files { unpack: self }
    }

    /// Verify every declared file was yielded. Call after consuming `files()`
    /// to surface `IntegrityKind::FileMissing` for any manifest entry that
    /// wasn't present in the zip.
    pub fn finalize(&self) -> Result<(), BundleError> {
        for path in self.declared.keys() {
            if !self.seen.contains_key(path) {
                return Err(BundleError::Integrity {
                    kind: IntegrityKind::FileMissing,
                    detail: path.clone(),
                });
            }
        }
        Ok(())
    }

    /// Convenience: collect every file into a BTreeMap, running `finalize()`
    /// at the end. Test-ergonomics shim — do not use on hot paths (defeats
    /// the streaming memory property).
    pub fn into_map(mut self) -> Result<(Manifest, BTreeMap<String, Vec<u8>>), BundleError> {
        let mut out = BTreeMap::new();
        loop {
            let next = self.files().next();
            match next {
                Some(Ok(f)) => {
                    out.insert(f.path, f.bytes);
                }
                Some(Err(e)) => return Err(e),
                None => break,
            }
        }
        self.finalize()?;
        Ok((self.manifest, out))
    }
}

pub struct Files<'u, 'a> {
    unpack: &'u mut Unpack<'a>,
}

impl<'u, 'a> Iterator for Files<'u, 'a> {
    type Item = Result<UnpackedFile, BundleError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let idx = self.unpack.next_index;
            if idx >= self.unpack.zip.len() {
                return None;
            }
            self.unpack.next_index += 1;

            let mut entry = match self.unpack.zip.by_index(idx) {
                Ok(e) => e,
                Err(e) => return Some(Err(e.into())),
            };
            let name = entry.name().to_string();
            if name == "manifest.json" {
                continue;
            }

            if let Err(e) = validate_path(&name) {
                return Some(Err(e));
            }

            let mut bytes = Vec::new();
            if let Err(e) = entry.read_to_end(&mut bytes) {
                return Some(Err(e.into()));
            }

            let expected = match self.unpack.declared.get(&name) {
                Some(h) => *h,
                None => {
                    return Some(Err(BundleError::Integrity {
                        kind: IntegrityKind::FileOrphan,
                        detail: name,
                    }));
                }
            };
            let actual = sha256_of(&bytes);
            if actual != expected {
                return Some(Err(BundleError::Integrity {
                    kind: IntegrityKind::HashMismatch,
                    detail: format!(
                        "{}: manifest={}, actual={}",
                        name,
                        hex::encode(expected),
                        hex::encode(actual)
                    ),
                }));
            }

            self.unpack.seen.insert(name.clone(), ());
            return Some(Ok(UnpackedFile { path: name, bytes }));
        }
    }
}

fn validate_manifest_semantics(m: &Manifest) -> Result<(), BundleError> {
    if m.schema_version != 1 {
        return Err(BundleError::Integrity {
            kind: IntegrityKind::SchemaVersionUnsupported,
            detail: format!("schema_version {}", m.schema_version),
        });
    }
    if !m.entry_overlay.ends_with(".omni") {
        return Err(BundleError::Unsafe {
            kind: UnsafeKind::Path,
            detail: format!("entry_overlay not .omni: {}", m.entry_overlay),
        });
    }
    for e in &m.files {
        validate_path(&e.path)?;
    }
    validate_manifest_references(m)
}
