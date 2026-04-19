//! Synthetic author keypair cache for dev seeding.
//!
//! Two fixtures: `alice` (display name "dev-alice") + `bob` ("dev-bob"). Each
//! has an Ed25519 keypair generated on first run via the shipped `identity`
//! crate; subsequent runs detect the `<slug>.key` file and reuse. Keypairs
//! persist across `omni-dev reset` so fixture artifacts remain authored by
//! stable synthetic identities.
//!
//! The keyfile format is the same as the host/user identity — encrypted seed
//! file produced by `identity::Keypair::load_or_create` — with `<slug>.pub.json`
//! a sidecar containing `{ display_name, pubkey_hex }` for easy reading.

// Consumer lands in T6 (seed) — the module is dead code until then, so
// silence `dead_code` here to stay clippy-clean under `-D warnings`.
#![allow(dead_code)]

use anyhow::Context;
use identity::Keypair;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureAuthor {
    pub slug: String,
    pub display_name: String,
    pub pubkey_hex: String,
}

const FIXTURES: &[(&str, &str)] = &[("alice", "dev-alice"), ("bob", "dev-bob")];

pub fn ensure_fixture_authors(dir: &Path) -> anyhow::Result<Vec<FixtureAuthor>> {
    fs::create_dir_all(dir).with_context(|| format!("create {:?}", dir))?;
    let mut out = Vec::new();
    for (slug, display_name) in FIXTURES {
        let key_path = dir.join(format!("{slug}.key"));
        let meta_path = dir.join(format!("{slug}.pub.json"));
        // Use identity's load_or_create so the keyfile format matches host +
        // admin. Returned Keypair exposes public_key() for the hex derivation.
        let kp = Keypair::load_or_create(&key_path)
            .with_context(|| format!("load or create {:?}", key_path))?;
        let pubkey_hex = hex::encode(kp.public_key().0);
        let author = FixtureAuthor {
            slug: slug.to_string(),
            display_name: display_name.to_string(),
            pubkey_hex: pubkey_hex.clone(),
        };
        // (Re)write the sidecar so consumers (seed) can cheaply look up
        // display_name + pubkey_hex without re-deriving.
        let meta_json =
            serde_json::to_vec_pretty(&author).context("serialize fixture author meta")?;
        fs::write(&meta_path, meta_json).with_context(|| format!("write {:?}", meta_path))?;
        out.push(author);
    }
    Ok(out)
}

pub fn load_fixture_author(dir: &Path, slug: &str) -> anyhow::Result<FixtureAuthor> {
    let meta_path = dir.join(format!("{slug}.pub.json"));
    let bytes = fs::read(&meta_path).with_context(|| format!("read {:?}", meta_path))?;
    let author: FixtureAuthor =
        serde_json::from_slice(&bytes).with_context(|| format!("parse {:?}", meta_path))?;
    Ok(author)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_fixture_authors_generates_alice_and_bob() {
        let tmp = TempDir::new().unwrap();
        let authors = ensure_fixture_authors(tmp.path()).unwrap();
        assert_eq!(authors.len(), 2);
        assert_eq!(authors[0].slug, "alice");
        assert_eq!(authors[0].display_name, "dev-alice");
        assert_eq!(authors[1].slug, "bob");
        assert_eq!(authors[1].display_name, "dev-bob");
        assert!(authors[0].pubkey_hex.len() == 64);
        assert!(authors[1].pubkey_hex.len() == 64);
        assert_ne!(authors[0].pubkey_hex, authors[1].pubkey_hex);
    }

    #[test]
    fn ensure_fixture_authors_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let first = ensure_fixture_authors(tmp.path()).unwrap();
        let second = ensure_fixture_authors(tmp.path()).unwrap();
        assert_eq!(first[0].pubkey_hex, second[0].pubkey_hex);
        assert_eq!(first[1].pubkey_hex, second[1].pubkey_hex);
    }

    #[test]
    fn load_fixture_author_reads_sidecar() {
        let tmp = TempDir::new().unwrap();
        let gen = ensure_fixture_authors(tmp.path()).unwrap();
        let loaded = load_fixture_author(tmp.path(), "alice").unwrap();
        assert_eq!(loaded.pubkey_hex, gen[0].pubkey_hex);
        assert_eq!(loaded.display_name, "dev-alice");
    }

    #[test]
    fn load_fixture_author_errors_when_missing() {
        let tmp = TempDir::new().unwrap();
        let err = load_fixture_author(tmp.path(), "alice").unwrap_err();
        assert!(err.to_string().contains("read"));
    }
}
