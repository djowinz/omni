//! `omni-admin artifact` — theme artifact moderation (show, remove).
//!
//! Wire contract: `worker-api.md` §4.16.
//!
//! - `show <id>`                         → `GET  /v1/artifact/:id`
//! - `remove <id> --reason <text>`       → `POST /v1/admin/artifact/:id/remove`
//!
//! `show` hits the public read endpoint; the admin CLI always signs so we
//! keep the same `send_signed` path. `remove` is state-changing and appends
//! a line to the local audit log (`~/.omni-admin/audit.log`) per sub-spec §6.

use clap::{Args as ClapArgs, Subcommand};
use std::process::ExitCode;

#[derive(ClapArgs, Debug, Clone)]
pub struct Args {
    #[command(subcommand)]
    pub sub: Sub,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Sub {
    /// Show a single artifact by id.
    Show { id: String },
    /// Remove (tombstone) an artifact with a mandatory reason.
    Remove {
        id: String,
        #[arg(long)]
        reason: String,
    },
}

pub async fn run(args: Args, cli: &crate::Cli) -> anyhow::Result<ExitCode> {
    let client = crate::client::AdminClient::from_cli(cli)?;
    match args.sub {
        Sub::Show { id } => {
            let path = format!("/v1/artifact/{id}");
            let v: serde_json::Value = client
                .send_signed(reqwest::Method::GET, &path, None, None, &[])
                .await?;
            crate::client::print_value(cli, &v);
        }
        Sub::Remove { id, reason } => {
            let body = serde_json::json!({ "reason": reason });
            let body_bytes = serde_json::to_vec(&body)?;
            let path = format!("/v1/admin/artifact/{id}/remove");
            let v: serde_json::Value = client
                .send_signed(reqwest::Method::POST, &path, None, Some(&body_bytes), &[])
                .await?;
            crate::audit::append(&format!(
                "REMOVE artifact={id} reason=\"{}\"",
                crate::audit::escape_value(&reason)
            ))?;
            crate::client::print_value(cli, &v);
        }
    }
    Ok(ExitCode::SUCCESS)
}
