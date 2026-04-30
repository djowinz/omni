//! Download + verify bundled moderation ONNX models per the manifest.
//!
//! Reads `crates/moderation/resources/MODELS.toml`, and for each entry:
//! 1. If the destination file already exists with a matching SHA-256, skip.
//! 2. Otherwise download from `url`, verify SHA-256 + size, write to disk.
//!
//! Idempotent — safe to run repeatedly. Canonical invocation is
//! `cargo run --release -p fetch-models` from anywhere in the workspace.
//! Run by:
//! - dev contributors before their first `cargo test -p moderation` /
//!   `-p host` (or before `cargo run -p host` in dev)
//! - CI as an explicit step before any host/moderation tests
//! - the release pipeline before electron-builder packages the installer
//!
//! Why a script and not Git LFS:
//! - LFS bandwidth quotas hit fast under any meaningful CI volume
//! - Models live in third-party-hostable space (Hugging Face, GitHub Releases)
//!   so the canonical source is outside the repo
//! - Repo stays tiny; model swaps don't bloat git history forever
//!
//! Exit codes:
//! - 0 — all models present and verified
//! - 1 — manifest read / parse error, network failure, or hash mismatch

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Manifest entry — one per bundled model. Top-level keys in `MODELS.toml` are
/// arbitrary identifiers (e.g. `nudenet`, `falconsai`); the `filename` field
/// determines the on-disk name under the manifest's parent directory.
#[derive(Debug, Deserialize)]
struct ModelEntry {
    /// Output filename relative to the manifest's parent directory.
    filename: String,
    /// HTTPS download URL. Following redirects is on by default in `ureq`.
    url: String,
    /// Lowercase hex SHA-256 of the expected bytes. Verified after download
    /// AND used to short-circuit re-download when the file already matches.
    sha256: String,
    /// Expected file size in bytes — sanity check against the URL serving a
    /// completely different file (e.g. an HTML 404 page).
    size: u64,
}

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(flatten)]
    models: std::collections::BTreeMap<String, ModelEntry>,
}

fn main() -> ExitCode {
    let manifest_path = match find_manifest() {
        Some(p) => p,
        None => {
            eprintln!(
                "fetch-models: could not locate crates/moderation/resources/MODELS.toml \
                 — run from anywhere inside the omni workspace"
            );
            return ExitCode::from(1);
        }
    };
    let manifest_dir = manifest_path.parent().expect("manifest has a parent dir");
    println!("fetch-models: manifest = {}", manifest_path.display());

    let manifest_text = match fs::read_to_string(&manifest_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fetch-models: read {}: {e}", manifest_path.display());
            return ExitCode::from(1);
        }
    };
    let manifest: Manifest = match toml::from_str(&manifest_text) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("fetch-models: parse {}: {e}", manifest_path.display());
            return ExitCode::from(1);
        }
    };

    let mut had_failure = false;
    for (name, entry) in &manifest.models {
        let dest = manifest_dir.join(&entry.filename);
        match ensure_one(name, entry, &dest) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("fetch-models: {name}: {e}");
                had_failure = true;
            }
        }
    }

    if had_failure {
        ExitCode::from(1)
    } else {
        println!("fetch-models: all models present and verified");
        ExitCode::SUCCESS
    }
}

/// Download + verify one model entry, skipping if already present + correct.
fn ensure_one(name: &str, entry: &ModelEntry, dest: &Path) -> Result<(), String> {
    if dest.exists() {
        let existing = sha256_of(dest)?;
        if existing.eq_ignore_ascii_case(&entry.sha256) {
            println!(
                "fetch-models: {name}: already present + verified ({})",
                dest.display()
            );
            return Ok(());
        }
        println!(
            "fetch-models: {name}: hash mismatch on disk, re-downloading from {}",
            entry.url
        );
    } else {
        println!(
            "fetch-models: {name}: not present, downloading from {}",
            entry.url
        );
    }

    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }

    // Headers required by the upstream hosts:
    // - `User-Agent`: HuggingFace + many CDNs reject empty UA.
    // - `Accept: application/octet-stream`: required by GitHub's
    //   `api.github.com/repos/.../releases/assets/<id>` endpoint to return the
    //   raw binary instead of a JSON description. Harmless for HF and other
    //   hosts that already serve octet-stream by default.
    let response = ureq::get(&entry.url)
        .set("User-Agent", "omni-fetch-models/0.1")
        .set("Accept", "application/octet-stream")
        .call()
        .map_err(|e| format!("HTTP error fetching {}: {e}", entry.url))?;

    let mut bytes: Vec<u8> = Vec::with_capacity(entry.size as usize);
    response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("read body of {}: {e}", entry.url))?;

    if (bytes.len() as u64) != entry.size {
        return Err(format!(
            "downloaded size {} bytes, manifest expected {} bytes — refusing to write",
            bytes.len(),
            entry.size
        ));
    }

    let actual_hash = hex_sha256(&bytes);
    if !actual_hash.eq_ignore_ascii_case(&entry.sha256) {
        return Err(format!(
            "downloaded SHA-256 {} != manifest {} — refusing to write",
            actual_hash, entry.sha256
        ));
    }

    fs::write(dest, &bytes).map_err(|e| format!("write {}: {e}", dest.display()))?;
    println!(
        "fetch-models: {name}: wrote {} bytes to {} (sha256 verified)",
        bytes.len(),
        dest.display()
    );
    Ok(())
}

/// Walk up from CWD looking for `crates/moderation/resources/MODELS.toml`.
/// The models are owned by the `moderation` crate (it loads them, its tests
/// reference them); the desktop installer mirrors them into its install
/// resources at packaging time via `electron-builder.yml`. This walk lets
/// the tool run from the workspace root or any nested location.
fn find_manifest() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir
            .join("crates")
            .join("moderation")
            .join("resources")
            .join("MODELS.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        let parent = dir.parent()?.to_path_buf();
        if parent == dir {
            return None;
        }
        dir = parent;
    }
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    Ok(hex_sha256(&bytes))
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        write!(&mut out, "{:02x}", b).expect("write to String");
    }
    out
}
