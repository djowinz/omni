//! `omni-admin artifact` — theme artifact moderation (show, remove).
//!
//! Stub — real implementation lands in Task 15.

use clap::Args as ClapArgs;

#[derive(ClapArgs, Debug, Clone)]
pub struct Args {}

pub async fn run(_args: Args, _cli: &crate::Cli) -> anyhow::Result<std::process::ExitCode> {
    anyhow::bail!("not yet implemented");
}
