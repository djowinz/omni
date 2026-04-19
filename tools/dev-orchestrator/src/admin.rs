//! `omni-dev admin -- <args>` forwards to the shipped `omni-admin` CLI with
//! the dev admin keypair + local worker URL pre-set. Stdio is inherited so
//! interactive subcommands (like `review`) work out of the box.

use crate::paths;
use anyhow::Context;
use std::process::Command;

pub fn run(forwarded: Vec<String>) -> anyhow::Result<()> {
    let ctx = paths::default_ctx();
    let admin_key = paths::admin_key_path(&ctx)?;
    if !admin_key.exists() {
        anyhow::bail!(
            "admin key not found at {}. Run `omni-dev reset-identity --which admin` first.",
            admin_key.display()
        );
    }

    let mut cmd = Command::new("cargo");
    cmd.args(["run", "--quiet", "-p", "admin", "--", "--key-file"])
        .arg(&admin_key)
        .args(["--worker-url", "http://127.0.0.1:8787"])
        .args(&forwarded);

    let status = cmd.status().context("spawn omni-admin")?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn admin_module_compiles() {
        // Functional testing of the subprocess happens at T10 manual smoke —
        // spawning cargo in unit tests is too slow + flaky.
    }
}
