//! `omni-admin reports` — inspect and act on user-submitted reports.
//!
//! Stub — real implementation lands in Task 14.

use clap::Args as ClapArgs;

#[derive(ClapArgs, Debug)]
pub struct Args {}

pub async fn run(_args: Args, _cli: &crate::Cli) -> anyhow::Result<std::process::ExitCode> {
    anyhow::bail!("not yet implemented");
}
