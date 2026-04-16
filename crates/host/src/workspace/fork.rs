//! Fork an installed bundle into a new local overlay, atomically.
//!
//! Reads `bundles/<slug>/`, writes `overlays/<name>/` via
//! `workspace::atomic_dir`, and records heritage in `.omni-origin.json`.

/// Windows reserved filename stems, uppercase. Match is case-insensitive and
/// applies whether or not the name carries an extension (per Win32 rules).
const WINDOWS_RESERVED_STEMS: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM0", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT0", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Validate a user-chosen overlay name.
pub(crate) fn sanitize_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("name must not be empty");
    }
    if name.chars().count() > 48 {
        return Err("name exceeds 48 characters");
    }
    if name != name.trim() {
        return Err("name must not have leading or trailing whitespace");
    }
    if name == "." || name == ".." {
        return Err("name must not be '.' or '..'");
    }
    for ch in name.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0' => {
                return Err("name contains a forbidden character");
            }
            c if c.is_control() => {
                return Err("name contains a non-printable character");
            }
            _ => {}
        }
    }
    // Windows reserved stems: compare the part before the first '.' (if any),
    // case-insensitive.
    let stem = name.split('.').next().unwrap_or(name);
    let stem_upper = stem.to_ascii_uppercase();
    if WINDOWS_RESERVED_STEMS.iter().any(|r| *r == stem_upper) {
        return Err("name is a Windows reserved stem");
    }
    Ok(())
}

use serde::{Deserialize, Serialize};

/// Written to `<overlay>/.omni-origin.json` on fork. The file's presence is
/// the heritage marker; there is no parallel registry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkOrigin {
    /// Schema version; bump on breaking changes.
    pub version: u32,
    pub forked_from: ForkSource,
    pub trust: ForkTrust,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkSource {
    /// `<author-slug>/<name>`, matches the installed-bundles registry id.
    pub artifact_id: String,
    pub content_hash: String,
    pub bundle_name: String,
    /// Hex-encoded Ed25519 pubkey of the original author.
    pub author_pubkey: String,
    pub author_display_name: Option<String>,
    pub author_fingerprint: String,
    /// Unix seconds at time of fork.
    pub forked_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkTrust {
    LocalAuthored,
}

pub(crate) const ORIGIN_SCHEMA_VERSION: u32 = 1;
pub(crate) const ORIGIN_FILE_NAME: &str = ".omni-origin.json";

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize_name;

    #[test]
    fn accepts_reasonable_names() {
        for good in ["my-hud", "Cyberpunk HUD", "a", "with_underscore",
                     "unicode-Ω-ok", "digits-123", "dot.in.middle"] {
            assert!(sanitize_name(good).is_ok(), "expected ok: {good:?}");
        }
    }

    #[test]
    fn rejects_empty_and_length_bounds() {
        assert!(sanitize_name("").is_err());
        let long = "x".repeat(49);
        assert!(sanitize_name(&long).is_err());
        let ok48 = "x".repeat(48);
        assert!(sanitize_name(&ok48).is_ok());
    }

    #[test]
    fn rejects_whitespace_edges() {
        for bad in [" leading", "trailing ", " both ", "\ttab\t"] {
            assert!(sanitize_name(bad).is_err(), "expected err: {bad:?}");
        }
    }

    #[test]
    fn rejects_dot_dotdot() {
        assert!(sanitize_name(".").is_err());
        assert!(sanitize_name("..").is_err());
    }

    #[test]
    fn rejects_path_traversal_and_separators() {
        for bad in ["../evil", "foo/bar", "foo\\bar", "/abs", "\\abs",
                    "c:name", "ads:stream"] {
            assert!(sanitize_name(bad).is_err(), "expected err: {bad:?}");
        }
    }

    #[test]
    fn rejects_forbidden_chars() {
        for bad in ["star*name", "q?mark", "quo\"te", "less<than",
                    "greater>than", "pipe|name"] {
            assert!(sanitize_name(bad).is_err(), "expected err: {bad:?}");
        }
    }

    #[test]
    fn rejects_null_and_control_bytes() {
        assert!(sanitize_name("nul\0byte").is_err());
        assert!(sanitize_name("bell\x07").is_err());
        assert!(sanitize_name("newline\nhere").is_err());
    }

    #[test]
    fn rejects_all_windows_reserved_stems_all_case_variants_and_with_ext() {
        let bases = [
            "CON", "PRN", "AUX", "NUL",
            "COM0", "COM1", "COM2", "COM3", "COM4",
            "COM5", "COM6", "COM7", "COM8", "COM9",
            "LPT0", "LPT1", "LPT2", "LPT3", "LPT4",
            "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];
        let case_variants = |s: &str| -> Vec<String> {
            vec![
                s.to_ascii_uppercase(),
                s.to_ascii_lowercase(),
                {
                    let mut c = s.chars();
                    match c.next() {
                        Some(first) => format!("{}{}",
                            first.to_ascii_uppercase(),
                            c.as_str().to_ascii_lowercase()),
                        None => String::new(),
                    }
                },
                s.chars().enumerate().map(|(i, c)| {
                    if i % 2 == 0 { c.to_ascii_lowercase() }
                    else { c.to_ascii_uppercase() }
                }).collect(),
            ]
        };
        for base in bases {
            for v in case_variants(base) {
                assert!(sanitize_name(&v).is_err(),
                    "expected err for reserved stem {v:?}");
                for ext in [".txt", ".omni", ".json"] {
                    let with_ext = format!("{v}{ext}");
                    assert!(sanitize_name(&with_ext).is_err(),
                        "expected err for reserved+ext {with_ext:?}");
                }
            }
        }
    }

    #[test]
    fn allows_reserved_stem_as_substring_but_not_as_stem() {
        assert!(sanitize_name("console").is_ok());
        assert!(sanitize_name("comic").is_ok());
        assert!(sanitize_name("lptop").is_ok());
        assert!(sanitize_name("con.anything").is_err());
    }
}

#[cfg(test)]
mod origin_tests {
    use super::*;

    fn sample() -> ForkOrigin {
        ForkOrigin {
            version: ORIGIN_SCHEMA_VERSION,
            forked_from: ForkSource {
                artifact_id: "lx92/cyberpunk-hud".into(),
                content_hash: "a".repeat(64),
                bundle_name: "cyberpunk-hud".into(),
                author_pubkey: "b".repeat(64),
                author_display_name: Some("LX92".into()),
                author_fingerprint: "c".repeat(8),
                forked_at: 1_700_000_000,
            },
            trust: ForkTrust::LocalAuthored,
        }
    }

    #[test]
    fn origin_json_roundtrip() {
        let o = sample();
        let s = serde_json::to_string_pretty(&o).expect("ser");
        let back: ForkOrigin = serde_json::from_str(&s).expect("de");
        assert_eq!(o, back);
    }

    #[test]
    fn origin_json_has_expected_snake_case_trust() {
        let o = sample();
        let s = serde_json::to_string(&o).unwrap();
        assert!(s.contains("\"trust\":\"local_authored\""), "was: {s}");
    }

    #[test]
    fn origin_missing_display_name_serdes() {
        let mut o = sample();
        o.forked_from.author_display_name = None;
        let s = serde_json::to_string(&o).unwrap();
        let back: ForkOrigin = serde_json::from_str(&s).unwrap();
        assert_eq!(o, back);
    }
}

use std::path::{Path, PathBuf};
use thiserror::Error;

/// `TargetExists` is the pre-check; `AtomicCommitFailed` is the commit-time
/// race where another process created the target between stage and rename.
#[derive(Debug, Error)]
pub enum ForkError {
    #[error("source bundle {0:?} is not installed")]
    SourceNotFound(String),

    #[error("overlay name is invalid: {0}")]
    NameInvalid(&'static str),

    #[error("overlay {0:?} already exists")]
    TargetExists(String),

    #[error("atomic commit failed")]
    AtomicCommitFailed(#[source] std::io::Error),

    #[error("failed to write .omni-origin.json")]
    OriginWriteFailed(#[source] std::io::Error),

    #[error("failed to serialize .omni-origin.json")]
    OriginSerdeFailed(#[source] serde_json::Error),

    #[error("unsupported file type in source bundle ({0})")]
    UnsupportedSourceEntry(String),

    #[error("io error")]
    Io(#[from] std::io::Error),
}

impl ForkError {
    /// Stable WebSocket error code for this variant.
    pub fn ws_error_code(&self) -> &'static str {
        match self {
            ForkError::NameInvalid(_) => "NAME_INVALID",
            ForkError::TargetExists(_) => "TARGET_EXISTS",
            ForkError::SourceNotFound(_) => "BUNDLE_NOT_INSTALLED",
            ForkError::AtomicCommitFailed(_) => "ATOMIC_COMMIT_FAILED",
            ForkError::OriginWriteFailed(_)
            | ForkError::OriginSerdeFailed(_)
            | ForkError::UnsupportedSourceEntry(_)
            | ForkError::Io(_) => "IO_ERROR",
        }
    }
}

pub struct ForkRequest {
    pub bundle_slug: String,
    pub new_overlay_name: String,
}

#[derive(Debug, Clone)]
pub struct ForkResult {
    pub path: PathBuf,
    pub name: String,
    pub origin: ForkOrigin,
}

/// Minimum surface fork needs from the installed-bundles registry.
pub trait InstalledBundleLookup {
    fn lookup(&self, slug: &str) -> Option<InstalledBundleView>;
}

#[derive(Debug, Clone)]
pub struct InstalledBundleView {
    pub path: PathBuf,
    pub artifact_id: String,
    pub content_hash: String,
    pub bundle_name: String,
    pub author_pubkey: String,
    pub author_display_name: Option<String>,
    pub author_fingerprint: String,
}

use crate::workspace::atomic_dir::AtomicDir;

/// Copy an installed bundle into a new overlay directory.
pub fn fork_to_local(
    req: ForkRequest,
    overlays_root: &Path,
    installed: &dyn InstalledBundleLookup,
) -> Result<ForkResult, ForkError> {
    sanitize_name(&req.new_overlay_name).map_err(ForkError::NameInvalid)?;

    let source = installed
        .lookup(&req.bundle_slug)
        .ok_or_else(|| ForkError::SourceNotFound(req.bundle_slug.clone()))?;

    let target = overlays_root.join(&req.new_overlay_name);
    if target.exists() {
        return Err(ForkError::TargetExists(req.new_overlay_name.clone()));
    }

    let staging = AtomicDir::stage(&target)?;

    copy_dir_recursive(&source.path, staging.path())?;

    let origin = ForkOrigin {
        version: ORIGIN_SCHEMA_VERSION,
        forked_from: ForkSource {
            artifact_id: source.artifact_id.clone(),
            content_hash: source.content_hash.clone(),
            bundle_name: source.bundle_name.clone(),
            author_pubkey: source.author_pubkey.clone(),
            author_display_name: source.author_display_name.clone(),
            author_fingerprint: source.author_fingerprint.clone(),
            forked_at: unix_now_secs(),
        },
        trust: ForkTrust::LocalAuthored,
    };
    let origin_path = staging.path().join(ORIGIN_FILE_NAME);
    {
        // Scope the writer so the file handle closes before commit()'s
        // rename — on Windows an open handle blocks directory rename.
        let file = std::fs::File::create(&origin_path)
            .map_err(ForkError::OriginWriteFailed)?;
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, &origin)
            .map_err(ForkError::OriginSerdeFailed)?;
        use std::io::Write;
        writer.flush().map_err(ForkError::OriginWriteFailed)?;
    }

    staging.commit(false).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            ForkError::AtomicCommitFailed(e)
        } else {
            ForkError::Io(e)
        }
    })?;

    Ok(ForkResult {
        path: target,
        name: req.new_overlay_name,
        origin,
    })
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), ForkError> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_dir_recursive(&from, &to)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to)?;
        } else {
            // Symlinks/special files should never reach here (sanitize
            // rejects them at install time); fail loudly if one does.
            return Err(ForkError::UnsupportedSourceEntry(
                from.display().to_string(),
            ));
        }
    }
    Ok(())
}

fn unix_now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod fork_tests {
    use super::*;
    use std::collections::HashMap;

    struct StubRegistry(HashMap<String, InstalledBundleView>);
    impl InstalledBundleLookup for StubRegistry {
        fn lookup(&self, slug: &str) -> Option<InstalledBundleView> {
            self.0.get(slug).cloned()
        }
    }

    fn make_installed_bundle(root: &Path, slug: &str) -> InstalledBundleView {
        let bdir = root.join("bundles").join(slug);
        std::fs::create_dir_all(bdir.join("themes")).unwrap();
        std::fs::write(bdir.join("overlay.omni"), b"<html>hi</html>").unwrap();
        std::fs::write(bdir.join("themes/dark.css"), b"body{}").unwrap();
        std::fs::write(bdir.join("manifest.json"), br#"{"name":"x"}"#).unwrap();
        InstalledBundleView {
            path: bdir,
            artifact_id: format!("auth/{slug}"),
            content_hash: "a".repeat(64),
            bundle_name: slug.into(),
            author_pubkey: "b".repeat(64),
            author_display_name: Some("Author".into()),
            author_fingerprint: "c".repeat(8),
        }
    }

    fn registry_with(root: &Path, slug: &str) -> (StubRegistry, PathBuf) {
        let view = make_installed_bundle(root, slug);
        let overlays = root.join("overlays");
        std::fs::create_dir_all(&overlays).unwrap();
        let mut m = HashMap::new();
        m.insert(slug.to_string(), view);
        (StubRegistry(m), overlays)
    }

    #[test]
    fn happy_path_copies_files_and_writes_origin() {
        let root = tempfile::TempDir::new().unwrap();
        let (reg, overlays) = registry_with(root.path(), "bundle-a");
        let out = fork_to_local(
            ForkRequest {
                bundle_slug: "bundle-a".into(),
                new_overlay_name: "my-copy".into(),
            },
            &overlays,
            &reg,
        ).expect("fork ok");
        assert_eq!(out.name, "my-copy");
        assert!(out.path.join("overlay.omni").exists());
        assert!(out.path.join("themes/dark.css").exists());
        assert!(out.path.join("manifest.json").exists());
        let origin_bytes = std::fs::read(out.path.join(".omni-origin.json")).unwrap();
        let parsed: ForkOrigin = serde_json::from_slice(&origin_bytes).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.forked_from.artifact_id, "auth/bundle-a");
        assert!(matches!(parsed.trust, ForkTrust::LocalAuthored));
    }

    #[test]
    fn returns_target_exists_when_overlay_present() {
        let root = tempfile::TempDir::new().unwrap();
        let (reg, overlays) = registry_with(root.path(), "bundle-a");
        std::fs::create_dir_all(overlays.join("collide")).unwrap();
        let err = fork_to_local(
            ForkRequest {
                bundle_slug: "bundle-a".into(),
                new_overlay_name: "collide".into(),
            },
            &overlays,
            &reg,
        ).unwrap_err();
        assert!(matches!(err, ForkError::TargetExists(ref n) if n == "collide"));
    }

    #[test]
    fn returns_source_not_found_for_unknown_slug() {
        let root = tempfile::TempDir::new().unwrap();
        let (reg, overlays) = registry_with(root.path(), "bundle-a");
        let err = fork_to_local(
            ForkRequest {
                bundle_slug: "nope".into(),
                new_overlay_name: "ok".into(),
            },
            &overlays,
            &reg,
        ).unwrap_err();
        assert!(matches!(err, ForkError::SourceNotFound(ref s) if s == "nope"));
    }

    #[test]
    fn returns_name_invalid_on_bad_name() {
        let root = tempfile::TempDir::new().unwrap();
        let (reg, overlays) = registry_with(root.path(), "bundle-a");
        let err = fork_to_local(
            ForkRequest {
                bundle_slug: "bundle-a".into(),
                new_overlay_name: "../evil".into(),
            },
            &overlays,
            &reg,
        ).unwrap_err();
        assert!(matches!(err, ForkError::NameInvalid(_)));
    }

    #[test]
    fn source_bundle_directory_unchanged_after_fork() {
        let root = tempfile::TempDir::new().unwrap();
        let (reg, overlays) = registry_with(root.path(), "bundle-a");
        let source = reg.0.get("bundle-a").unwrap().path.clone();
        let before: Vec<_> = walk(&source);
        fork_to_local(
            ForkRequest {
                bundle_slug: "bundle-a".into(),
                new_overlay_name: "copy".into(),
            },
            &overlays,
            &reg,
        ).unwrap();
        let after: Vec<_> = walk(&source);
        assert_eq!(before, after);
    }

    fn walk(p: &Path) -> Vec<(PathBuf, u64)> {
        let mut v = Vec::new();
        walk_inner(p, &mut v);
        v.sort();
        v
    }
    fn walk_inner(p: &Path, out: &mut Vec<(PathBuf, u64)>) {
        if let Ok(rd) = std::fs::read_dir(p) {
            for e in rd.flatten() {
                let path = e.path();
                if let Ok(md) = e.metadata() {
                    if md.is_file() {
                        out.push((path.clone(), md.len()));
                    } else if md.is_dir() {
                        walk_inner(&path, out);
                    }
                }
            }
        }
    }
}
