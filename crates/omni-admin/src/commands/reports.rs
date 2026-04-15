//! `omni-admin reports` — list, show, and act on user-submitted reports.
//!
//! Wire contract: `worker-api.md` §4.13–§4.15.
//!
//! - `list [--status …] [--limit N] [--cursor C]` → `GET /v1/admin/reports`
//! - `show <id>`                                  → `GET /v1/admin/report/:id`
//! - `action <id> --action … [--notes …]`         → `POST /v1/admin/report/:id/action`
//!
//! Every state-changing `action` call appends a line to the local audit log
//! (`~/.omni-admin/audit.log`) per sub-spec §6 — the Worker is authoritative,
//! the local log is forensic "what did I do yesterday?".

use clap::{Args as ClapArgs, Subcommand, ValueEnum};
use std::process::ExitCode;

#[derive(ClapArgs, Debug, Clone)]
pub struct Args {
    #[command(subcommand)]
    pub sub: Sub,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Sub {
    /// List reports, optionally filtered by status, with pagination.
    List {
        #[arg(long)]
        status: Option<Status>,
        #[arg(long, default_value_t = 50)]
        limit: u32,
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Show a single report by id.
    Show {
        id: String,
    },
    /// Act on a report (close with a resolution).
    Action {
        id: String,
        #[arg(long)]
        action: ActionKind,
        #[arg(long)]
        notes: Option<String>,
    },
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum Status {
    Pending,
    Reviewed,
    Actioned,
}

#[derive(ValueEnum, Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    NoAction,
    Removed,
    BannedAuthor,
}

pub async fn run(args: Args, cli: &crate::Cli) -> anyhow::Result<ExitCode> {
    let client = crate::client::AdminClient::from_cli(cli)?;
    match args.sub {
        Sub::List {
            status,
            limit,
            cursor,
        } => {
            let mut q: Vec<String> = Vec::new();
            if let Some(s) = status {
                q.push(format!("status={}", status_str(s)));
            }
            q.push(format!("limit={limit}"));
            if let Some(c) = cursor {
                q.push(format!("cursor={}", urlencoding::encode(&c)));
            }
            let query = q.join("&");
            let v: serde_json::Value = client
                .send_signed(
                    reqwest::Method::GET,
                    "/v1/admin/reports",
                    Some(&query),
                    None,
                    &[],
                )
                .await?;
            print_json_or_human(cli, &v);
        }
        Sub::Show { id } => {
            let path = format!("/v1/admin/report/{id}");
            let v: serde_json::Value = client
                .send_signed(reqwest::Method::GET, &path, None, None, &[])
                .await?;
            print_json_or_human(cli, &v);
        }
        Sub::Action { id, action, notes } => {
            let body = serde_json::json!({ "action": action, "notes": notes });
            let body_bytes = serde_json::to_vec(&body)?;
            let path = format!("/v1/admin/report/{id}/action");
            let v: serde_json::Value = client
                .send_signed(
                    reqwest::Method::POST,
                    &path,
                    None,
                    Some(&body_bytes),
                    &[],
                )
                .await?;
            crate::audit::append(&format!(
                "ACTION report={id} action={action:?} notes={}",
                notes.as_deref().unwrap_or("")
            ))?;
            print_json_or_human(cli, &v);
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn status_str(s: Status) -> &'static str {
    match s {
        Status::Pending => "pending",
        Status::Reviewed => "reviewed",
        Status::Actioned => "actioned",
    }
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
