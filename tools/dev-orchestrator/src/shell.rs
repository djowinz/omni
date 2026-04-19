//! Cross-platform shell spawn helpers.
//!
//! Rust's `Command::new("pnpm")` / `Command::new("node")` do NOT resolve
//! Windows `.cmd` / `.bat` shims (`pnpm` ships as `pnpm.cmd`), unlike Node's
//! `spawn(..., { shell: true })` which auto-handles PATHEXT. These helpers
//! route through `cmd /C` on Windows and `sh -c` on Unix so the shell
//! performs the lookup.

use std::process::Command as StdCommand;
use tokio::process::Command as TokioCommand;

/// Async (tokio) shell-routed `Command` builder.
/// Usage:
/// ```no_run
/// # use dev_orchestrator::shell;
/// let mut cmd = shell::tokio_cmd("pnpm exec wrangler dev");
/// let child = cmd.spawn();
/// ```
pub fn tokio_cmd(script: &str) -> TokioCommand {
    let mut cmd = if cfg!(windows) {
        TokioCommand::new("cmd")
    } else {
        TokioCommand::new("sh")
    };
    if cfg!(windows) {
        cmd.args(["/C", script]);
    } else {
        cmd.args(["-c", script]);
    }
    cmd
}

/// Sync (std) shell-routed `Command` builder for subprocess-driven helpers
/// like `seed` and `reset` that use `spawn_sync` / `output`-style semantics.
pub fn std_cmd(script: &str) -> StdCommand {
    let mut cmd = if cfg!(windows) {
        StdCommand::new("cmd")
    } else {
        StdCommand::new("sh")
    };
    if cfg!(windows) {
        cmd.args(["/C", script]);
    } else {
        cmd.args(["-c", script]);
    }
    cmd
}
