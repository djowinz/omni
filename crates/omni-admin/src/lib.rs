//! `omni-admin` — operator CLI for the themes worker moderation surface.
//!
//! This crate is scaffolded by Task 10 of the theme-sharing #012 plan. The
//! top-level `Cli` / `Cmd` types live here (not in `main.rs`) so integration
//! tests and future subagent task work can import them as
//! `omni_admin::{Cli, Cmd}` without poking into the binary crate.

use clap::{Parser, Subcommand};

pub mod auth;
pub mod audit;
pub mod client;
pub mod commands;
pub mod key_file;

/// Top-level CLI entry. Global flags apply to every subcommand.
#[derive(Parser, Debug)]
#[command(name = "omni-admin", version, about = "Omni themes-worker operator CLI")]
pub struct Cli {
    /// Path to the operator key file (Ed25519 signing key, ChaCha20-Poly1305 wrapped).
    #[arg(long, global = true)]
    pub key_file: Option<std::path::PathBuf>,

    /// Base URL of the themes worker.
    #[arg(
        long,
        global = true,
        env = "OMNI_ADMIN_WORKER_URL",
        default_value = "https://themes.omni.prod/"
    )]
    pub worker_url: String,

    /// Skip interactive confirmation prompts (dangerous — scripts only).
    #[arg(long, global = true)]
    pub yes: bool,

    /// Emit machine-readable JSON output instead of human-friendly text.
    #[arg(long, global = true)]
    pub json: bool,

    #[command(subcommand)]
    pub cmd: Cmd,
}

/// Subcommands. Each variant carries its own `Args` struct declared in the
/// matching `commands::<name>` module. T13–T20 replace those stubs.
#[derive(Subcommand, Debug)]
pub enum Cmd {
    /// Generate a new operator key file.
    Keygen(commands::keygen::Args),
    /// Inspect and act on user-submitted reports.
    Reports(commands::reports::Args),
    /// Manage theme artifacts (takedown, restore, inspect).
    Artifact(commands::artifact::Args),
    /// Manage the operator public-key allowlist.
    Pubkey(commands::pubkey::Args),
    /// Device / client moderation (bans, lookups).
    Device(commands::device::Args),
    /// Manage the sanitization vocabulary.
    Vocab(commands::vocab::Args),
    /// Inspect and adjust rate-limit state.
    Limits(commands::limits::Args),
    /// Review-queue operations.
    Review(commands::review::Args),
    /// Dashboard / stats queries.
    Stats(commands::stats::Args),
}
