//! `omni-admin vocab` — sanitization vocabulary management.
//!
//! Stub — real implementation lands in Task 18.

use clap::Args as ClapArgs;

#[derive(ClapArgs, Debug)]
pub struct Args {}

pub async fn run(_args: Args, _cli: &crate::Cli) -> anyhow::Result<std::process::ExitCode> {
    anyhow::bail!("not yet implemented");
}
