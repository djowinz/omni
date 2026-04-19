//! Cross-platform subprocess spawn helpers.
//!
//! Rust's `Command::new("pnpm")` does NOT resolve Windows `.cmd` / `.bat`
//! shims (`pnpm` ships as `pnpm.cmd`), unlike Node's `spawn({ shell: true })`
//! which auto-handles PATHEXT. These helpers append the correct shim
//! extension on Windows so the binary resolves via PATH.
//!
//! We intentionally do NOT route through `cmd /C "<script>"` because that
//! forces the entire command line into a single shell-parsed string, and
//! Windows cmd's quoting rules leak double-quotes into argv (breaking
//! file-path args). By calling the shim directly and passing args as a
//! vec, Rust's std handles per-arg quoting correctly.
//!
//! Use:
//!   shell::tokio_cmd("pnpm", &["exec", "wrangler", "dev"])
//!   shell::std_cmd("node", &["scripts/bootstrap-kv.mjs", "--local"])

use std::process::Command as StdCommand;
use tokio::process::Command as TokioCommand;

/// Resolve the platform-specific executable name for a command. On Windows,
/// shell shims like `pnpm` / `npm` / `npx` ship as `<name>.cmd` files that
/// Rust won't auto-resolve without the extension. Tools distributed as
/// native `.exe` (e.g. `cargo`, `node`, `rustc`) don't need this.
pub fn resolve_shim(program: &str) -> String {
    #[cfg(windows)]
    {
        match program {
            "pnpm" | "npm" | "npx" | "yarn" => format!("{program}.cmd"),
            other => other.to_string(),
        }
    }
    #[cfg(not(windows))]
    {
        program.to_string()
    }
}

pub fn tokio_cmd<I, S>(program: &str, args: I) -> TokioCommand
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut cmd = TokioCommand::new(resolve_shim(program));
    cmd.args(args);
    cmd
}

pub fn std_cmd<I, S>(program: &str, args: I) -> StdCommand
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut cmd = StdCommand::new(resolve_shim(program));
    cmd.args(args);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(windows)]
    fn resolve_shim_appends_cmd_on_windows_for_known_shims() {
        assert_eq!(resolve_shim("pnpm"), "pnpm.cmd");
        assert_eq!(resolve_shim("npm"), "npm.cmd");
        assert_eq!(resolve_shim("npx"), "npx.cmd");
        assert_eq!(resolve_shim("yarn"), "yarn.cmd");
    }

    #[test]
    #[cfg(windows)]
    fn resolve_shim_leaves_exe_tools_alone() {
        assert_eq!(resolve_shim("cargo"), "cargo");
        assert_eq!(resolve_shim("node"), "node");
        assert_eq!(resolve_shim("git"), "git");
    }

    #[test]
    #[cfg(not(windows))]
    fn resolve_shim_is_identity_on_unix() {
        assert_eq!(resolve_shim("pnpm"), "pnpm");
        assert_eq!(resolve_shim("node"), "node");
    }
}
