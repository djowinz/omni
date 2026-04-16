//! `omni-admin pubkey` — author public-key moderation (ban, unban).
//!
//! Wire contract: `worker-api.md` §4.17.
//!
//! - `ban <pubkey_hex> --reason <text> [--confirm-cascade]`
//!   → `POST /v1/admin/pubkey/ban`  `{ pubkey, reason }`
//! - `unban <pubkey_hex>`
//!   → `POST /v1/admin/pubkey/unban` `{ pubkey }`
//!
//! Banning an author cascades server-side: every artifact signed by that
//! pubkey is tombstoned. The response echoes `cascade_count` /
//! `cascade_errors`; we record both in the local audit log per sub-spec §6.
//! Because the blast radius is large, we require an interactive confirmation
//! unless `--confirm-cascade` (scripted) or the global `--yes` is supplied.

use clap::{Args as ClapArgs, Subcommand};
use std::process::ExitCode;

#[derive(ClapArgs, Debug, Clone)]
pub struct Args {
    #[command(subcommand)]
    pub sub: Sub,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Sub {
    /// Ban an author pubkey; cascades tombstones to all their artifacts.
    Ban {
        pubkey: String,
        #[arg(long)]
        reason: String,
        /// Skip the interactive cascade confirmation (non-interactive scripts).
        #[arg(long)]
        confirm_cascade: bool,
    },
    /// Lift a pubkey ban. No cascade: previously tombstoned artifacts stay down.
    Unban { pubkey: String },
}

pub async fn run(args: Args, cli: &crate::Cli) -> anyhow::Result<ExitCode> {
    let client = crate::client::AdminClient::from_cli(cli)?;
    match args.sub {
        Sub::Ban {
            pubkey,
            reason,
            confirm_cascade,
        } => {
            if !confirm_cascade && !cli.yes {
                let proceed = dialoguer::Confirm::new()
                    .with_prompt(format!(
                        "Ban pubkey {pubkey}? This will tombstone all of the author's artifacts."
                    ))
                    .default(false)
                    .interact()?;
                if !proceed {
                    eprintln!("aborted");
                    return Ok(ExitCode::from(1));
                }
            }
            let body = serde_json::json!({ "pubkey": pubkey, "reason": reason });
            let body_bytes = serde_json::to_vec(&body)?;
            let v: serde_json::Value = client
                .send_signed(
                    reqwest::Method::POST,
                    "/v1/admin/pubkey/ban",
                    None,
                    Some(&body_bytes),
                    &[],
                )
                .await?;
            let cascade_count = v.get("cascade_count").and_then(|n| n.as_u64()).unwrap_or(0);
            let cascade_errors = v
                .get("cascade_errors")
                .and_then(|n| n.as_u64())
                .unwrap_or(0);
            crate::audit::append(&format!(
                "BAN pubkey={pubkey} reason=\"{}\" cascade_count={cascade_count} cascade_errors={cascade_errors}",
                crate::audit::escape_value(&reason)
            ))?;
            crate::client::print_value(cli, &v);
        }
        Sub::Unban { pubkey } => {
            let body = serde_json::json!({ "pubkey": pubkey });
            let body_bytes = serde_json::to_vec(&body)?;
            let v: serde_json::Value = client
                .send_signed(
                    reqwest::Method::POST,
                    "/v1/admin/pubkey/unban",
                    None,
                    Some(&body_bytes),
                    &[],
                )
                .await?;
            crate::audit::append(&format!("UNBAN pubkey={pubkey}"))?;
            crate::client::print_value(cli, &v);
        }
    }
    Ok(ExitCode::SUCCESS)
}
