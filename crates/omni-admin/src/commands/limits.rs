//! `omni-admin limits` — rate-limit inspection / adjustment.
//!
//! Wire contract: `worker-api.md` §4.12 `PATCH /v1/admin/limits` and the
//! public `GET /v1/config/limits`.
//!
//! - `get`               → `GET   /v1/config/limits`
//! - `set [--fields] [--force]` → `PATCH /v1/admin/limits`
//!
//! `--force` sends `X-Omni-Admin-Force: true` to override the Worker's
//! `WouldOrphanArtifacts` safety when shrinking limits below existing
//! artifact sizes. Mutations append a single-line audit record with the
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
    /// Fetch current rate-limit configuration.
    Get,
    /// Adjust one or more rate limits. At least one `--max-*` flag required.
    Set {
        #[arg(long)]
        max_bundle_compressed: Option<u64>,
        #[arg(long)]
        max_bundle_uncompressed: Option<u64>,
        #[arg(long)]
        max_entries: Option<u64>,
        /// Override WouldOrphanArtifacts safety (sends X-Omni-Admin-Force: true).
        #[arg(long)]
        force: bool,
    },
}

pub async fn run(args: Args, cli: &crate::Cli) -> anyhow::Result<ExitCode> {
    let client = crate::client::AdminClient::from_cli(cli)?;
    match args.sub {
        Sub::Get => {
            let v: serde_json::Value = client
                .send_signed(reqwest::Method::GET, "/v1/config/limits", None, None, &[])
                .await?;
            print_json_or_human(cli, &v);
        }
        Sub::Set {
            max_bundle_compressed,
            max_bundle_uncompressed,
            max_entries,
            force,
        } => {
            if max_bundle_compressed.is_none()
                && max_bundle_uncompressed.is_none()
                && max_entries.is_none()
            {
                anyhow::bail!(
                    "at least one of --max-bundle-compressed, --max-bundle-uncompressed, --max-entries required"
                );
            }
            let mut body = serde_json::Map::new();
            if let Some(n) = max_bundle_compressed {
                body.insert("max_bundle_compressed".into(), n.into());
            }
            if let Some(n) = max_bundle_uncompressed {
                body.insert("max_bundle_uncompressed".into(), n.into());
            }
            if let Some(n) = max_entries {
                body.insert("max_entries".into(), n.into());
            }
            let body_bytes = serde_json::to_vec(&serde_json::Value::Object(body.clone()))?;
            let extra: &[(&str, &str)] = if force {
                &[("X-Omni-Admin-Force", "true")]
            } else {
                &[]
            };
            let v: serde_json::Value = client
                .send_signed(
                    reqwest::Method::PATCH,
                    "/v1/admin/limits",
                    None,
                    Some(&body_bytes),
                    extra,
                )
                .await?;
            let version_after = v.get("version").and_then(|n| n.as_u64()).unwrap_or(0);
            let mut parts: Vec<String> =
                body.iter().map(|(k, val)| format!("{k}={val}")).collect();
            parts.push(format!("version_after={version_after}"));
            crate::audit::append(&format!("LIMITS {}", parts.join(" ")))?;
            print_json_or_human(cli, &v);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_json_or_human(cli: &crate::Cli, v: &serde_json::Value) {
    if cli.json {
        println!("{v}");
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
        );
    }
}
