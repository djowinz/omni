//! `omni-dev` — one-command local dev stack orchestrator for the Omni
//! theme-sharing product. See
//! `docs/superpowers/plans/2026-04-18-local-dev-worker-rust.md` for the full
//! design.

use clap::{Parser, Subcommand};

mod admin;
mod fixtures;
mod identity_mgmt;
mod kill;
mod paths;

#[derive(Parser, Debug)]
#[command(name = "omni-dev", about = "Local dev stack for Omni", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start the full dev stack: wrangler + Electron (which spawns the host).
    Run {
        #[arg(long)]
        no_seed: bool,
    },
    /// Seed (or re-seed) the local miniflare D1 + R2 with fixture data.
    Seed,
    /// Wipe miniflare state, re-migrate, re-bootstrap, optionally re-seed.
    Reset {
        #[arg(long)]
        no_seed: bool,
    },
    /// Regenerate the dev user + admin keypairs.
    ResetIdentity {
        #[arg(long, value_parser = ["user", "admin", "both"], default_value = "both")]
        which: String,
    },
    /// Proxy: forwards to `omni-admin` with the dev key-file + local worker URL.
    Admin {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Force-kill any process bound to the known dev ports (8787, 9473).
    Kill,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Run { no_seed: _ } => {
            todo!("orchestrator::run — landed in T8")
        }
        Command::Seed => {
            todo!("seed — landed in T6")
        }
        Command::Reset { no_seed: _ } => {
            todo!("reset — landed in T7")
        }
        Command::ResetIdentity { which } => {
            identity_mgmt::reset(identity_mgmt::Which::from_str(&which))?;
            Ok(())
        }
        Command::Admin { args } => admin::run(args),
        Command::Kill => {
            kill::kill_all()?;
            Ok(())
        }
    }
}
