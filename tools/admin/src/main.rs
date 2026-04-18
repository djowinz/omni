//! `omni-admin` binary entry point.
//!
//! The `Cli` / `Cmd` types and all real logic live in the library crate
//! (`omni_admin`) so integration tests (Task 11+) can link them. This file
//! is intentionally a two-liner that parses and dispatches.

use clap::Parser;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> anyhow::Result<ExitCode> {
    let cli = omni_admin::Cli::parse();
    omni_admin::commands::dispatch(cli).await
}
