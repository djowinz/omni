//! Subcommand modules. Each `<name>.rs` owns a `pub struct Args` and a
//! `pub async fn run(args, cli) -> anyhow::Result<ExitCode>`. T13–T20 fill
//! these in; T10 leaves them as `bail!("not yet implemented")` stubs.

use std::process::ExitCode;

use crate::{Cli, Cmd};

pub mod artifact;
pub mod device;
pub mod keygen;
pub mod limits;
pub mod pubkey;
pub mod reports;
pub mod review;
pub mod stats;
pub mod vocab;

/// Dispatch a parsed `Cli` to the appropriate subcommand handler.
///
/// Handlers receive their parsed `Args` by value and a borrow of the full
/// `Cli` for access to the global flags (`--key-file`, `--worker-url`,
/// `--yes`, `--json`). We clone the `Cmd` so we can hand each handler an
/// owned `Args` while still passing `&cli` — clap-parsed `Args` types
/// derive `Clone`, so this is cheap and avoids placeholder values that
/// break every time a subcommand's `Args` gains a required field.
pub async fn dispatch(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.cmd.clone() {
        Cmd::Keygen(a) => keygen::run(a, &cli).await,
        Cmd::Reports(a) => reports::run(a, &cli).await,
        Cmd::Artifact(a) => artifact::run(a, &cli).await,
        Cmd::Pubkey(a) => pubkey::run(a, &cli).await,
        Cmd::Device(a) => device::run(a, &cli).await,
        Cmd::Vocab(a) => vocab::run(a, &cli).await,
        Cmd::Limits(a) => limits::run(a, &cli).await,
        Cmd::Review(a) => review::run(a, &cli).await,
        Cmd::Stats(a) => stats::run(a, &cli).await,
    }
}
