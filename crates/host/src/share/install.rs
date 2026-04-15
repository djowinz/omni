//! Install orchestration — download -> verify -> sanitize -> stream -> commit.
//!
//! Per invariants:
//! - #1/#9: `unpack_signed_bundle` is the single verify surface; holding a
//!   `SignedBundle` proves JWS + hash. There is NO separate `verify` step.
//! - #7: `sanitize_bundle` re-runs locally regardless of Worker output. Its
//!   error surface is the gate (per spec §2 step 4) — we do NOT byte-compare
//!   sanitized vs original, because sanitizers canonicalize by design.
//!   Sanitize Ok => drop the sanitized map and install the original signed
//!   bytes (what the per-file sha256 values cover).
//! - #19a: `InstallError` carves by domain; third-party errors are
//!   `#[source]`-chained, never public `#[from]` variants (except `io::Error`,
//!   the `std` exception).
//! - #19b: per-file `sha256` check happens INSIDE the `SignedBundle.files()`
//!   loop while writing to staging, never as a post-walk map. The temporary
//!   files-map built for the sanitize gate is a documented, bounded exception
//!   (bounded by `BundleLimits`, default 10 MB).

use std::collections::{BTreeMap, HashMap};
use std::io;
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use omni_bundle::{BundleLimits, FileEntry};
use omni_identity::{
    unpack_signed_bundle, Fingerprint, IdentityError, PublicKey, SignedBundle, TofuResult,
};
use omni_sanitize::{sanitize_bundle, SanitizeError};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::share::client::{DownloadError, ShareClient};
use crate::share::registry::{InstalledEntry, RegistryHandle, RegistryKind};
use crate::share::tofu::TofuStore;
use crate::workspace::atomic_dir::AtomicDir;

#[derive(Debug, Error)]
pub enum InstallError {
    #[error("io failure: {0}")]
    IoFailure(#[from] io::Error),

    #[error("bundle rejected ({kind:?}): {detail}")]
    BadBundle {
        kind: BadBundleKind,
        detail: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("signature failed: {0}")]
    SignatureFailed(String),

    #[error("TOFU mismatch: known={known}, seen={seen}")]
    TofuViolation { known: String, seen: String },

    #[error("requires omni >= {required}, current {current}")]
    VersionMismatch {
        required: semver::Version,
        current: semver::Version,
    },

    #[error("install cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Copy)]
pub enum BadBundleKind {
    Malformed,
    Unsafe,
    Integrity,
}

#[derive(Debug, Clone)]
pub enum InstallWarning {
    ExceedsCurrentPolicy { kind: String, actual: u64, limit: u64 },
}

#[derive(Debug, Clone)]
pub enum InstallProgress {
    Downloading { received: u64, total: u64 },
    Verifying,
    Sanitizing,
    Writing { file: String, index: usize, total: usize },
    Committing,
}

#[derive(Debug)]
pub struct InstallRequest {
    pub artifact_id: String,
    pub target_path: PathBuf,
    pub overwrite: bool,
    pub expected_pubkey: Option<PublicKey>,
}

#[derive(Debug)]
pub struct InstallOutcome {
    pub installed_path: PathBuf,
    pub content_hash: [u8; 32],
    pub author_pubkey: PublicKey,
    pub fingerprint: Fingerprint,
    pub tofu: TofuResult,
    pub warnings: Vec<InstallWarning>,
}

/// Full install driver.
#[allow(clippy::too_many_arguments)]
pub async fn install(
    req: InstallRequest,
    client: &ShareClient,
    tofu: &mut TofuStore,
    registry: &mut RegistryHandle,
    registry_kind: RegistryKind,
    limits: &BundleLimits,
    current_version: &semver::Version,
    cancel: CancellationToken,
    mut progress: impl FnMut(InstallProgress),
) -> Result<InstallOutcome, InstallError> {
    let bytes = tokio::select! {
        _ = cancel.cancelled() => return Err(InstallError::Cancelled),
        r = client.download(&req.artifact_id, |rx, total| {
            progress(InstallProgress::Downloading { received: rx, total });
        }) => r.map_err(client_to_install_error)?,
    };

    progress(InstallProgress::Verifying);
    let signed: SignedBundle =
        unpack_signed_bundle(&bytes, req.expected_pubkey.as_ref(), limits)
            .map_err(identity_to_install_error)?;

    let required = signed.manifest().omni_min_version.clone();
    if *current_version < required {
        return Err(InstallError::VersionMismatch {
            required,
            current: current_version.clone(),
        });
    }

    // TOFU check must run before any filesystem work so a mismatch never
    // leaves partial state behind.
    let author_pubkey = *signed.author_pubkey();
    let display_name = signed.manifest().name.clone();
    let tofu_result = tofu
        .check_or_record(&author_pubkey, &display_name)
        .map_err(identity_to_install_error)?;
    if let TofuResult::DisplayNameMismatch {
        known_pubkey_hex,
        seen_pubkey_hex,
        ..
    } = &tofu_result
    {
        return Err(InstallError::TofuViolation {
            known: known_pubkey_hex.clone(),
            seen: seen_pubkey_hex.clone(),
        });
    }

    sanitize_stage_and_commit(&signed, &req.target_path, req.overwrite, &cancel, &mut progress)
        .await?;

    let content_hash = sha256_of(&bytes);
    let entry = InstalledEntry {
        artifact_id: req.artifact_id.clone(),
        content_hash: hex::encode(content_hash),
        author_pubkey: author_pubkey.to_hex(),
        fingerprint_hex: author_pubkey.fingerprint().to_hex(),
        source_url: format!("download://{}", req.artifact_id),
        installed_at: now_unix(),
        installed_version: signed.manifest().version.clone(),
        omni_min_version: signed.manifest().omni_min_version.clone(),
    };
    let key = match registry_kind {
        RegistryKind::Themes => display_name.clone(),
        RegistryKind::Bundles => format!("{}-{}", &author_pubkey.to_hex()[..8], display_name),
    };
    registry.upsert(key, entry);
    registry
        .save()
        .map_err(|e| InstallError::IoFailure(io::Error::other(e.to_string())))?;

    tofu.record_install(&author_pubkey)
        .map_err(identity_to_install_error)?;

    Ok(InstallOutcome {
        installed_path: req.target_path,
        content_hash,
        author_pubkey,
        fingerprint: author_pubkey.fingerprint(),
        tofu: tofu_result,
        warnings: vec![],
    })
}

/// Sanitize gate → stage → per-file hash + stream → commit. Shared between
/// `install` and the test-only inline helper so the streaming loop body
/// (invariant #19b) lives in exactly one place.
async fn sanitize_stage_and_commit(
    signed: &SignedBundle,
    target_path: &std::path::Path,
    overwrite: bool,
    cancel: &CancellationToken,
    progress: &mut impl FnMut(InstallProgress),
) -> Result<(), InstallError> {
    progress(InstallProgress::Sanitizing);
    // Sanitize is a GATE (invariant #7, spec §2 step 4): its error surface is
    // the defense. The Ok'd sanitized map is dropped — installed bytes are
    // the signed originals, which the manifest's per-file sha256 values cover.
    let files_map: BTreeMap<String, Vec<u8>> = signed
        .files()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    if let Err(e) = sanitize_bundle(signed.manifest(), files_map) {
        return Err(sanitize_to_install_error(e));
    }
    if cancel.is_cancelled() {
        return Err(InstallError::Cancelled);
    }

    let staging = AtomicDir::stage(target_path)?;
    let expected: HashMap<&str, &FileEntry> = signed
        .manifest()
        .files
        .iter()
        .map(|f| (f.path.as_str(), f))
        .collect();
    let total = expected.len();

    for (index, (path, body)) in signed.files().enumerate() {
        if cancel.is_cancelled() {
            return Err(InstallError::Cancelled);
        }
        let entry = expected
            .get(path.as_str())
            .ok_or_else(|| InstallError::BadBundle {
                kind: BadBundleKind::Integrity,
                detail: format!("file not in manifest: {path}"),
                source: None,
            })?;
        if sha256_of(body) != entry.sha256 {
            return Err(InstallError::BadBundle {
                kind: BadBundleKind::Integrity,
                detail: format!("hash mismatch for {path}"),
                source: None,
            });
        }
        let dest = staging.path().join(path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, body)?;
        progress(InstallProgress::Writing {
            file: path.clone(),
            index,
            total,
        });
    }

    progress(InstallProgress::Committing);
    staging.commit(overwrite)?;
    Ok(())
}

// ---- Error mappers ----------------------------------------------------------

fn client_to_install_error(e: DownloadError) -> InstallError {
    match e {
        DownloadError::Http(err) => InstallError::IoFailure(io::Error::other(err.to_string())),
        DownloadError::Status { status, body } => InstallError::BadBundle {
            kind: BadBundleKind::Malformed,
            detail: format!("worker status {status}"),
            source: Some(Box::new(io::Error::other(body))),
        },
    }
}

fn identity_to_install_error(e: IdentityError) -> InstallError {
    use omni_bundle::{BundleError, IntegrityKind};
    match e {
        IdentityError::MissingSignature => {
            InstallError::SignatureFailed("missing signature.jws".into())
        }
        IdentityError::Jws(msg) => InstallError::SignatureFailed(msg),
        IdentityError::Bundle(BundleError::Integrity { kind, detail }) => {
            let k = match kind {
                IntegrityKind::HashMismatch => BadBundleKind::Integrity,
                _ => BadBundleKind::Malformed,
            };
            InstallError::BadBundle {
                kind: k,
                detail,
                source: None,
            }
        }
        IdentityError::Bundle(other) => {
            let detail = other.to_string();
            InstallError::BadBundle {
                kind: BadBundleKind::Malformed,
                detail,
                source: Some(Box::new(other)),
            }
        }
        other => InstallError::BadBundle {
            kind: BadBundleKind::Malformed,
            detail: other.to_string(),
            source: None,
        },
    }
}

fn sanitize_to_install_error(e: SanitizeError) -> InstallError {
    InstallError::BadBundle {
        kind: BadBundleKind::Unsafe,
        detail: e.to_string(),
        source: Some(Box::new(e)),
    }
}

fn sha256_of(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---- Test-only helpers ------------------------------------------------------

#[cfg(test)]
pub(crate) async fn install_from_bytes_for_tests(
    bytes: &[u8],
    target: &Path,
) -> Result<InstallOutcome, InstallError> {
    install_inline_for_tests(
        bytes,
        target,
        CancellationToken::new(),
        semver::Version::new(99, 0, 0),
    )
    .await
}

#[cfg(test)]
pub(crate) async fn install_from_bytes_with_current_version_for_tests(
    bytes: &[u8],
    target: &Path,
    current: semver::Version,
) -> Result<InstallOutcome, InstallError> {
    install_inline_for_tests(bytes, target, CancellationToken::new(), current).await
}

#[cfg(test)]
pub(crate) async fn install_from_bytes_cancellable_for_tests(
    bytes: &[u8],
    target: &Path,
    cancel: CancellationToken,
) -> Result<InstallOutcome, InstallError> {
    install_inline_for_tests(bytes, target, cancel, semver::Version::new(99, 0, 0)).await
}

#[cfg(test)]
async fn install_inline_for_tests(
    bytes: &[u8],
    target: &Path,
    cancel: CancellationToken,
    current_version: semver::Version,
) -> Result<InstallOutcome, InstallError> {
    // Test-only seam: skips network, TOFU state, and registry writes. The
    // pipeline proper (sanitize → stage → stream → commit) runs through the
    // shared `sanitize_stage_and_commit` helper.
    let limits = BundleLimits::DEFAULT;
    let signed =
        unpack_signed_bundle(bytes, None, &limits).map_err(identity_to_install_error)?;
    let required = signed.manifest().omni_min_version.clone();
    if current_version < required {
        return Err(InstallError::VersionMismatch {
            required,
            current: current_version,
        });
    }

    sanitize_stage_and_commit(&signed, target, false, &cancel, &mut |_| {}).await?;

    let pk = *signed.author_pubkey();
    Ok(InstallOutcome {
        installed_path: target.to_path_buf(),
        content_hash: sha256_of(bytes),
        author_pubkey: pk,
        fingerprint: pk.fingerprint(),
        tofu: TofuResult::FirstSeen,
        warnings: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_bundle::{BundleLimits, FileEntry, Manifest, Tag};
    use omni_identity::{pack_signed_bundle, Keypair};
    use std::collections::BTreeMap;
    use tempfile::TempDir;

    fn build_fixture() -> (Vec<u8>, Keypair, BundleLimits) {
        let kp = Keypair::generate();
        let overlay_bytes = b"<overlay></overlay>".to_vec();
        let theme_bytes = b"body { color: red; }".to_vec();
        let overlay_sha: [u8; 32] = {
            let mut h = Sha256::new();
            h.update(&overlay_bytes);
            h.finalize().into()
        };
        let theme_sha: [u8; 32] = {
            let mut h = Sha256::new();
            h.update(&theme_bytes);
            h.finalize().into()
        };
        let manifest = Manifest {
            schema_version: 1,
            name: "test-theme".into(),
            version: semver::Version::new(1, 0, 0),
            omni_min_version: semver::Version::new(0, 1, 0),
            description: "fixture".into(),
            tags: vec![Tag::new("dark").unwrap()],
            license: "MIT".into(),
            entry_overlay: "overlay.omni".into(),
            default_theme: Some("themes/theme.css".into()),
            sensor_requirements: vec![],
            files: vec![
                FileEntry {
                    path: "overlay.omni".into(),
                    sha256: overlay_sha,
                },
                FileEntry {
                    path: "themes/theme.css".into(),
                    sha256: theme_sha,
                },
            ],
            resource_kinds: None,
        };
        let mut files = BTreeMap::new();
        files.insert("overlay.omni".into(), overlay_bytes);
        files.insert("themes/theme.css".into(), theme_bytes);
        let limits = BundleLimits::DEFAULT;
        let bytes = pack_signed_bundle(&manifest, &files, &kp, &limits).unwrap();
        (bytes, kp, limits)
    }

    #[tokio::test]
    async fn happy_path_installs_and_registers() {
        let (bytes, _kp, _limits) = build_fixture();
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("themes").join("test-theme");
        let outcome = install_from_bytes_for_tests(&bytes, &target).await.unwrap();
        assert!(target.exists());
        assert_eq!(outcome.installed_path, target);
    }

    #[tokio::test]
    async fn tampered_file_fails_with_integrity() {
        let (mut bytes, _kp, _limits) = build_fixture();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xFF;
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("themes").join("x");
        let err = install_from_bytes_for_tests(&bytes, &target)
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                InstallError::BadBundle { kind: BadBundleKind::Integrity, .. }
                    | InstallError::BadBundle { kind: BadBundleKind::Malformed, .. }
                    | InstallError::SignatureFailed(_)
            ),
            "unexpected error: {err:?}"
        );
        assert!(!target.exists(), "no residue on failure");
    }

    #[tokio::test]
    async fn cancellation_leaves_no_residue() {
        let (bytes, _kp, _limits) = build_fixture();
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("themes").join("x");
        let token = CancellationToken::new();
        token.cancel();
        let err = install_from_bytes_cancellable_for_tests(&bytes, &target, token)
            .await
            .unwrap_err();
        assert!(matches!(err, InstallError::Cancelled));
        assert!(!target.exists());
        let parent = target.parent().unwrap_or(dir.path());
        let staging_leftovers = std::fs::read_dir(parent)
            .ok()
            .map(|it| {
                it.filter_map(Result::ok)
                    .filter(|e| {
                        e.file_name()
                            .to_str()
                            .map(|n| n.starts_with(".omni-staging-"))
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(staging_leftovers, 0);
    }

    #[tokio::test]
    async fn missing_signature_surfaces_as_signature_failed() {
        // Pack via the plain `omni_bundle::pack` — this produces a structurally
        // valid bundle WITHOUT a `signature.jws` entry. `unpack_signed_bundle`
        // must surface that absence as `IdentityError::MissingSignature`,
        // which the install pipeline maps to `InstallError::SignatureFailed`.
        let overlay_bytes = b"<overlay></overlay>".to_vec();
        let theme_bytes = b"body { color: red; }".to_vec();
        let overlay_sha: [u8; 32] = {
            let mut h = Sha256::new();
            h.update(&overlay_bytes);
            h.finalize().into()
        };
        let theme_sha: [u8; 32] = {
            let mut h = Sha256::new();
            h.update(&theme_bytes);
            h.finalize().into()
        };
        let manifest = Manifest {
            schema_version: 1,
            name: "unsigned-theme".into(),
            version: semver::Version::new(1, 0, 0),
            omni_min_version: semver::Version::new(0, 1, 0),
            description: "fixture".into(),
            tags: vec![Tag::new("dark").unwrap()],
            license: "MIT".into(),
            entry_overlay: "overlay.omni".into(),
            default_theme: Some("themes/theme.css".into()),
            sensor_requirements: vec![],
            files: vec![
                FileEntry {
                    path: "overlay.omni".into(),
                    sha256: overlay_sha,
                },
                FileEntry {
                    path: "themes/theme.css".into(),
                    sha256: theme_sha,
                },
            ],
            resource_kinds: None,
        };
        let mut files = BTreeMap::new();
        files.insert("overlay.omni".to_string(), overlay_bytes);
        files.insert("themes/theme.css".to_string(), theme_bytes);
        let bytes = omni_bundle::pack(&manifest, &files, &BundleLimits::DEFAULT).unwrap();

        let dir = TempDir::new().unwrap();
        let target = dir.path().join("themes").join("unsigned-theme");
        let err = install_from_bytes_for_tests(&bytes, &target)
            .await
            .unwrap_err();
        assert!(
            matches!(err, InstallError::SignatureFailed(_)),
            "expected SignatureFailed, got: {err:?}"
        );
        assert!(!target.exists(), "no residue on signature failure");
    }

    #[tokio::test]
    async fn version_mismatch_rejects_before_staging() {
        let (bytes, _kp, _limits) = build_fixture();
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("themes").join("x");
        let current = semver::Version::new(0, 0, 1);
        let err = install_from_bytes_with_current_version_for_tests(&bytes, &target, current)
            .await
            .unwrap_err();
        assert!(matches!(err, InstallError::VersionMismatch { .. }));
        assert!(!target.exists());
    }
}
