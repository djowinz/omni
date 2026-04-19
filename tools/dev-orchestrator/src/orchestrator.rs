//! `omni-dev run` — orchestrate the full dev stack.
//!
//! Flow:
//!   1. Pre-flight: check host binary + keypairs. Auto-gen keys via
//!      identity_mgmt if missing.
//!   2. Read admin pubkey hex from the admin keyfile (no sidecar).
//!   3. Spawn `wrangler dev` with `--var OMNI_ADMIN_PUBKEYS:<hex>`.
//!   4. Wait for port 8787.
//!   5. Seed (unless --no-seed).
//!   6. Spawn `pnpm --filter @omni/desktop dev` with env vars set.
//!   7. Tee colored prefixed logs.
//!   8. Ctrl-C: kill children in reverse spawn order.

use crate::{fixtures, identity_mgmt, seed};
use anyhow::{anyhow, Context};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::signal;
use tokio::time::{sleep, Duration, Instant};

const WORKER_DIR: &str = "apps/worker";
const HOST_BIN_PATH: &str = "target/debug/omni-host.exe";

pub async fn run(skip_seed: bool) -> anyhow::Result<()> {
    preflight()?;
    identity_mgmt::ensure(identity_mgmt::Which::Both)?;

    let key_paths = identity_mgmt::key_paths()?;
    let admin_pubkey = identity_mgmt::read_pubkey_hex(&key_paths.admin)?
        .ok_or_else(|| anyhow!("admin key unexpectedly missing after ensure"))?;
    tracing::info!(%admin_pubkey, "admin pubkey ready");

    // Also ensure fixture authors exist up-front so seed doesn't race to create
    // them while the app is booting.
    fixtures::ensure_fixture_authors(Path::new("apps/worker/seed/dev-fixtures"))?;

    // Spawn wrangler dev.
    let mut wrangler = spawn_wrangler(&admin_pubkey).context("spawn wrangler dev")?;
    let wrangler_id = wrangler.id();

    // Tee its output.
    if let Some(stdout) = wrangler.stdout.take() {
        tokio::spawn(tee("[worker]", "\x1b[36m", stdout));
    }
    if let Some(stderr) = wrangler.stderr.take() {
        tokio::spawn(tee_err("[worker]", "\x1b[36m", stderr));
    }

    if !wait_for_port(8787, Duration::from_secs(30)).await {
        let _ = wrangler.kill().await;
        anyhow::bail!("wrangler dev did not start within 30s");
    }

    if !skip_seed {
        if let Err(e) = seed::run() {
            tracing::warn!(error = %e, "seed failed — continuing, worker stays running");
        }
    }

    // Spawn Electron with env.
    let identity_path_str = key_paths.user.to_string_lossy().to_string();
    let mut electron = Command::new("pnpm")
        .args(["--filter", "@omni/desktop", "dev"])
        .env("OMNI_WORKER_URL", "http://127.0.0.1:8787/")
        .env("OMNI_IDENTITY_PATH", &identity_path_str)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn Electron dev")?;
    let electron_id = electron.id();
    if let Some(stdout) = electron.stdout.take() {
        tokio::spawn(tee("[electron]", "\x1b[35m", stdout));
    }
    if let Some(stderr) = electron.stderr.take() {
        tokio::spawn(tee_err("[electron]", "\x1b[35m", stderr));
    }

    tracing::info!(wrangler_pid = ?wrangler_id, electron_pid = ?electron_id, "dev stack running");

    // Wait for Ctrl-C OR either child to exit.
    tokio::select! {
        _ = signal::ctrl_c() => {
            tracing::info!("Ctrl-C received; tearing down");
        }
        status = electron.wait() => {
            tracing::info!(status = ?status, "electron exited; tearing down");
        }
        status = wrangler.wait() => {
            tracing::info!(status = ?status, "wrangler exited; tearing down");
        }
    }

    // Teardown in reverse spawn order.
    let _ = electron.kill().await;
    let _ = wrangler.kill().await;

    Ok(())
}

/// `omni-dev worker` — start wrangler dev with admin-pubkey + seed, NO Electron.
///
/// Useful when iterating on worker code or hitting the local API from curl /
/// Postman / a browser without the Electron overhead. Same admin-keypair
/// injection + seed behavior as `run`, just skips the Electron half.
pub async fn worker(skip_seed: bool) -> anyhow::Result<()> {
    // No host-binary preflight needed — the host isn't spawned.
    identity_mgmt::ensure(identity_mgmt::Which::Admin)?;

    let key_paths = identity_mgmt::key_paths()?;
    let admin_pubkey = identity_mgmt::read_pubkey_hex(&key_paths.admin)?
        .ok_or_else(|| anyhow!("admin key unexpectedly missing after ensure"))?;
    tracing::info!(%admin_pubkey, "admin pubkey ready");

    fixtures::ensure_fixture_authors(Path::new("apps/worker/seed/dev-fixtures"))?;

    let mut wrangler = spawn_wrangler(&admin_pubkey).context("spawn wrangler dev")?;
    let wrangler_id = wrangler.id();

    if let Some(stdout) = wrangler.stdout.take() {
        tokio::spawn(tee("[worker]", "\x1b[36m", stdout));
    }
    if let Some(stderr) = wrangler.stderr.take() {
        tokio::spawn(tee_err("[worker]", "\x1b[36m", stderr));
    }

    if !wait_for_port(8787, Duration::from_secs(30)).await {
        let _ = wrangler.kill().await;
        anyhow::bail!("wrangler dev did not start within 30s");
    }

    if !skip_seed {
        if let Err(e) = seed::run() {
            tracing::warn!(error = %e, "seed failed — continuing, worker stays running");
        }
    }

    tracing::info!(wrangler_pid = ?wrangler_id, "wrangler running (worker-only mode)");

    tokio::select! {
        _ = signal::ctrl_c() => {
            tracing::info!("Ctrl-C received; shutting down wrangler");
        }
        status = wrangler.wait() => {
            tracing::info!(status = ?status, "wrangler exited");
        }
    }

    let _ = wrangler.kill().await;
    Ok(())
}

fn preflight() -> anyhow::Result<()> {
    if !Path::new(HOST_BIN_PATH).exists() {
        anyhow::bail!("host binary missing at {HOST_BIN_PATH} — run `cargo build -p host` first");
    }
    Ok(())
}

fn spawn_wrangler(admin_pubkey_hex: &str) -> std::io::Result<Child> {
    Command::new("pnpm")
        .args([
            "exec",
            "wrangler",
            "dev",
            "--var",
            &format!("OMNI_ADMIN_PUBKEYS:{admin_pubkey_hex}"),
        ])
        .current_dir(WORKER_DIR)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
}

async fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
            Ok(_) => return true,
            Err(_) => sleep(Duration::from_millis(300)).await,
        }
    }
    false
}

async fn tee<R>(prefix: &'static str, color: &'static str, pipe: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(pipe).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        println!("{color}{prefix}\x1b[0m {line}");
    }
}

async fn tee_err<R>(prefix: &'static str, color: &'static str, pipe: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(pipe).lines();
    while let Ok(Some(line)) = reader.next_line().await {
        eprintln!("{color}{prefix}\x1b[0m {line}");
    }
}

#[cfg(test)]
mod tests {
    // The orchestrator spawns real subprocesses; integration-test it at T10.
    // Unit tests here would duplicate what Wave A's module tests already cover.
}
