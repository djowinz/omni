//! Shared helpers for `crates/omni-admin/tests/*.rs` integration binaries.
//!
//! Each integration test binary in Rust is its own translation unit, so
//! historically each file re-declared the same `mint_key` helper. That
//! duplication is brittle (changes have to be ported to every binary) and
//! noisy (8× 8-line repeats). This module is included via `mod common;` in
//! every binary that needs it; `cargo test` discovers `tests/common/mod.rs`
//! naturally and never treats it as its own test binary.

use assert_cmd::Command;
use std::path::{Path, PathBuf};

/// Mint a fresh admin identity key under `dir` by invoking the `omni-admin
/// keygen` subcommand end-to-end. Returning the path lets the caller pass it
/// straight to `--key-file` without knowing the internal filename layout.
#[allow(dead_code)] // not every test binary uses this helper
pub fn mint_key(dir: &Path) -> PathBuf {
    let out = dir.join("admin-identity.key");
    Command::cargo_bin("admin")
        .unwrap()
        .args(["keygen", "--output"])
        .arg(&out)
        .assert()
        .success();
    out
}
