//! `omni-admin vocab` — sanitization vocabulary management.
//!
//! Wire contract: `worker-api.md` §4.11 `PATCH /v1/admin/vocab` and the
//! public `GET /v1/config/vocab`.
//!
//! - `list`            → `GET   /v1/config/vocab`
//! - `add    <tag>`    → `PATCH /v1/admin/vocab`  `{ add:    [tag] }`
//! - `remove <tag>`    → `PATCH /v1/admin/vocab`  `{ remove: [tag] }`
//!
//! All three paths go through `AdminClient::send_signed` — the Worker accepts
//! signed requests on public endpoints (contract §1), which keeps the client
//! path uniform. Mutations append a single-line audit record with the
//! post-mutation `version` returned by the Worker.

use clap::{Args as ClapArgs, Subcommand};
use std::process::ExitCode;

#[derive(ClapArgs, Debug, Clone)]
pub struct Args {
    #[command(subcommand)]
    pub sub: Sub,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Sub {
    /// List the current sanitization vocabulary.
    List,
    /// Add a tag to the vocabulary.
    Add { tag: String },
    /// Remove a tag from the vocabulary.
    Remove { tag: String },
}

pub async fn run(args: Args, cli: &crate::Cli) -> anyhow::Result<ExitCode> {
    let client = crate::client::AdminClient::from_cli(cli)?;
    match &args.sub {
        Sub::List => {
            let v: serde_json::Value = client
                .send_signed(reqwest::Method::GET, "/v1/config/vocab", None, None, &[])
                .await?;
            crate::client::print_value(cli, &v);
        }
        Sub::Add { tag } => {
            let body = serde_json::json!({ "add": [tag] });
            let body_bytes = serde_json::to_vec(&body)?;
            let v: serde_json::Value = client
                .send_signed(
                    reqwest::Method::PATCH,
                    "/v1/admin/vocab",
                    None,
                    Some(&body_bytes),
                    &[],
                )
                .await?;
            let version = v.get("version").and_then(|n| n.as_u64()).unwrap_or(0);
            crate::audit::append(&format!("VOCAB add={tag} version_after={version}"))?;
            crate::client::print_value(cli, &v);
        }
        Sub::Remove { tag } => {
            let body = serde_json::json!({ "remove": [tag] });
            let body_bytes = serde_json::to_vec(&body)?;
            let v: serde_json::Value = client
                .send_signed(
                    reqwest::Method::PATCH,
                    "/v1/admin/vocab",
                    None,
                    Some(&body_bytes),
                    &[],
                )
                .await?;
            let version = v.get("version").and_then(|n| n.as_u64()).unwrap_or(0);
            crate::audit::append(&format!("VOCAB remove={tag} version_after={version}"))?;
            crate::client::print_value(cli, &v);
        }
    }
    Ok(ExitCode::SUCCESS)
}
