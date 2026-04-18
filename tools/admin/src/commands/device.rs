//! `omni-admin device` — device/client fingerprint moderation (ban, unban).
//!
//! Wire contract: `worker-api.md` §4.18.
//!
//! - `ban <device_fp_hex> --reason <text>`
//!   → `POST /v1/admin/device/ban`  `{ device_fp, reason }`
//! - `unban <device_fp_hex>`
//!   → `POST /v1/admin/device/unban` `{ device_fp }`
//!
//! Unlike the pubkey variant, device bans do not cascade to artifacts, so
//! there is no interactive confirmation and no `--confirm-cascade` flag.

use clap::{Args as ClapArgs, Subcommand};
use std::process::ExitCode;

#[derive(ClapArgs, Debug, Clone)]
pub struct Args {
    #[command(subcommand)]
    pub sub: Sub,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Sub {
    /// Ban a device fingerprint. Does not cascade.
    Ban {
        device_fp: String,
        #[arg(long)]
        reason: String,
    },
    /// Lift a device fingerprint ban.
    Unban { device_fp: String },
}

pub async fn run(args: Args, cli: &crate::Cli) -> anyhow::Result<ExitCode> {
    let client = crate::client::AdminClient::from_cli(cli)?;
    match args.sub {
        Sub::Ban { device_fp, reason } => {
            let body = serde_json::json!({ "device_fp": device_fp, "reason": reason });
            let body_bytes = serde_json::to_vec(&body)?;
            let v: serde_json::Value = client
                .send_signed(
                    reqwest::Method::POST,
                    "/v1/admin/device/ban",
                    None,
                    Some(&body_bytes),
                    &[],
                )
                .await?;
            crate::audit::append(&format!(
                "BAN device={device_fp} reason=\"{}\"",
                crate::audit::escape_value(&reason)
            ))?;
            crate::client::print_value(cli, &v);
        }
        Sub::Unban { device_fp } => {
            let body = serde_json::json!({ "device_fp": device_fp });
            let body_bytes = serde_json::to_vec(&body)?;
            let v: serde_json::Value = client
                .send_signed(
                    reqwest::Method::POST,
                    "/v1/admin/device/unban",
                    None,
                    Some(&body_bytes),
                    &[],
                )
                .await?;
            crate::audit::append(&format!("UNBAN device={device_fp}"))?;
            crate::client::print_value(cli, &v);
        }
    }
    Ok(ExitCode::SUCCESS)
}
