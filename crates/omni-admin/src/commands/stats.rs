//! `omni-admin stats` — dashboard / stats queries.
//!
//! Wire contract: `worker-api.md` §4.19. Read-only, no audit entry.

use clap::Args as ClapArgs;
use std::process::ExitCode;

#[derive(ClapArgs, Debug, Clone)]
pub struct Args {}

pub async fn run(_args: Args, cli: &crate::Cli) -> anyhow::Result<ExitCode> {
    let client = crate::client::AdminClient::from_cli(cli)?;
    let v: serde_json::Value = client
        .send_signed(
            reqwest::Method::GET,
            "/v1/admin/stats",
            None,
            None,
            &[],
        )
        .await?;
    if cli.json {
        println!("{v}");
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
        );
    }
    Ok(ExitCode::SUCCESS)
}
