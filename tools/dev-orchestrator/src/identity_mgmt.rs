//! Dev user + admin keypair management.
//!
//! Reuses `identity::Keypair::load_or_create` — same format as the host user
//! identity and the `omni-admin keygen` output. Because we own both the
//! keyfile (Rust) and the crypto routines (Rust), reading the pubkey from an
//! existing keyfile is a single call — no sidecar needed.

// `ensure` + `read_pubkey_hex` are wired by T8's `orchestrator::run`; the whole
// module is dead code until that lands, so silence `dead_code` here to stay
// clippy-clean under `-D warnings`.
#![allow(dead_code)]

use crate::paths;
use anyhow::Context;
use identity::Keypair;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub enum Which {
    User,
    Admin,
    Both,
}

impl Which {
    pub fn from_str(s: &str) -> Self {
        match s {
            "user" => Which::User,
            "admin" => Which::Admin,
            _ => Which::Both,
        }
    }
}

pub struct KeyPaths {
    pub user: std::path::PathBuf,
    pub admin: std::path::PathBuf,
}

pub fn key_paths() -> anyhow::Result<KeyPaths> {
    let ctx = paths::default_ctx();
    Ok(KeyPaths {
        user: paths::identity_key_path(&ctx)?,
        admin: paths::admin_key_path(&ctx)?,
    })
}

/// Ensure the selected keys exist. Non-destructive — existing keys are kept.
pub fn ensure(which: Which) -> anyhow::Result<()> {
    let kp = key_paths()?;
    let targets = targets(which, &kp);
    for (name, path) in targets {
        fs::create_dir_all(path.parent().unwrap())
            .with_context(|| format!("create parent of {:?}", path))?;
        let existed = path.exists();
        let _ = Keypair::load_or_create(path)
            .with_context(|| format!("load/create {name} key at {:?}", path))?;
        if existed {
            tracing::info!(%name, path = %path.display(), "dev key already exists");
        } else {
            tracing::info!(%name, path = %path.display(), "generated dev key");
        }
    }
    Ok(())
}

/// Regenerate the selected keys — removes existing files first so load_or_create
/// produces fresh material.
pub fn reset(which: Which) -> anyhow::Result<()> {
    let kp = key_paths()?;
    let targets = targets(which, &kp);
    for (name, path) in targets {
        if path.exists() {
            fs::remove_file(path)
                .with_context(|| format!("remove existing {name} key at {:?}", path))?;
            tracing::info!(%name, "removed existing key");
        }
    }
    ensure(which)
}

/// Read the pubkey hex from an existing keyfile. Returns `None` if the key
/// file is missing.
pub fn read_pubkey_hex(path: &Path) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let kp = Keypair::load_or_create(path).with_context(|| format!("read {:?}", path))?;
    Ok(Some(hex::encode(kp.public_key().0)))
}

fn targets(which: Which, kp: &KeyPaths) -> Vec<(&'static str, &std::path::Path)> {
    let mut out = Vec::new();
    if matches!(which, Which::User | Which::Both) {
        out.push(("user", kp.user.as_path()));
    }
    if matches!(which, Which::Admin | Which::Both) {
        out.push(("admin", kp.admin.as_path()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn which_from_str_parses_all_variants() {
        matches!(Which::from_str("user"), Which::User);
        matches!(Which::from_str("admin"), Which::Admin);
        matches!(Which::from_str("both"), Which::Both);
        matches!(Which::from_str("unknown"), Which::Both); // fallback to both
    }

    #[test]
    fn read_pubkey_hex_returns_none_when_missing() {
        let tmp = TempDir::new().unwrap();
        let got = read_pubkey_hex(&tmp.path().join("nonexistent.key")).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn read_pubkey_hex_round_trip_via_load_or_create() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test.key");
        // Create the key via the same API production code uses.
        let kp = Keypair::load_or_create(&path).unwrap();
        let expected = hex::encode(kp.public_key().0);
        let got = read_pubkey_hex(&path).unwrap().unwrap();
        assert_eq!(got, expected);
    }
}
