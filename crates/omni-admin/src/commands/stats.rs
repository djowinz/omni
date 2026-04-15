//! `omni-admin stats` — dashboard / stats queries.
//!
//! Stub — real implementation lands in Task 20.

use clap::Args as ClapArgs;

#[derive(ClapArgs, Debug, Clone)]
pub struct Args {}

pub async fn run(_args: Args, _cli: &crate::Cli) -> anyhow::Result<std::process::ExitCode> {
    anyhow::bail!("not yet implemented");
}
