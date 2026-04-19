//! Reset the miniflare state directory, re-apply D1 migrations, re-run KV
//! bootstrap, optionally re-seed. Preserves identity keypairs (they live
//! outside the worker dir).

use crate::{seed, shell};
use anyhow::Context;
use std::fs;
use std::path::Path;

const WORKER_DIR: &str = "apps/worker";
const STATE_DIR: &str = "apps/worker/.wrangler/state";

pub fn run(skip_seed: bool) -> anyhow::Result<()> {
    wipe_state()?;
    run_d1_migrations()?;
    run_kv_bootstrap()?;
    if !skip_seed {
        seed::run()?;
    }
    tracing::info!("reset complete");
    Ok(())
}

fn wipe_state() -> anyhow::Result<()> {
    if Path::new(STATE_DIR).exists() {
        fs::remove_dir_all(STATE_DIR).with_context(|| format!("wipe {}", STATE_DIR))?;
        tracing::info!(state_dir = STATE_DIR, "wiped miniflare state");
    } else {
        tracing::info!(state_dir = STATE_DIR, "no state dir to wipe");
    }
    Ok(())
}

/// Apply pending D1 migrations to the local miniflare DB. Idempotent:
/// wrangler tracks applied migration IDs and skips ones already on record.
/// Exposed `pub(crate)` so the orchestrator can prime a fresh `.wrangler/state/`
/// before seeding.
pub(crate) fn run_d1_migrations() -> anyhow::Result<()> {
    tracing::info!("applying D1 migrations");
    let status = shell::std_cmd(
        "pnpm",
        [
            "exec",
            "wrangler",
            "d1",
            "migrations",
            "apply",
            "META",
            "--local",
        ],
    )
    .current_dir(WORKER_DIR)
    .status()?;
    if !status.success() {
        anyhow::bail!("wrangler d1 migrations apply failed");
    }
    Ok(())
}

/// Run the KV bootstrap script that seeds `config:vocab` + `config:limits`.
/// Idempotent (script overwrites the same keys each run). Exposed
/// `pub(crate)` so the orchestrator can prime a fresh `.wrangler/state/`.
pub(crate) fn run_kv_bootstrap() -> anyhow::Result<()> {
    tracing::info!("bootstrapping KV");
    let status = shell::std_cmd("node", ["scripts/bootstrap-kv.mjs", "--local"])
        .current_dir(WORKER_DIR)
        .status()?;
    if !status.success() {
        anyhow::bail!("bootstrap-kv failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // run() itself spawns wrangler, which is too slow + environment-dependent
    // for unit tests. State validation happens at T10 manual smoke.
}
