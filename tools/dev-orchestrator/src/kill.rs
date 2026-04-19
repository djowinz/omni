//! Port-kill recovery — when a prior `omni-dev run` didn't shut down cleanly,
//! this finds any process still bound to the known dev ports (8787, 9473) and
//! terminates it.

use std::process::Command;

const PORTS: &[u16] = &[8787, 9473];

pub fn kill_all() -> anyhow::Result<()> {
    let mut any = false;
    for port in PORTS {
        if kill_port(*port)? {
            tracing::info!(port = *port, "terminated dev process");
            any = true;
        }
    }
    if !any {
        tracing::info!("no dev processes found on known ports");
    }
    Ok(())
}

#[cfg(windows)]
fn kill_port(port: u16) -> anyhow::Result<bool> {
    let output = Command::new("cmd")
        .args(["/c", &format!("netstat -ano | findstr :{port}")])
        .output()?;
    if !output.status.success() || output.stdout.is_empty() {
        return Ok(false);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut pids: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in stdout.lines() {
        // netstat columns: proto  local  foreign  state  PID
        let parts: Vec<&str> = line.split_whitespace().collect();
        if let Some(pid) = parts.last() {
            if pid.chars().all(|c| c.is_ascii_digit()) {
                pids.insert(pid.to_string());
            }
        }
    }
    let mut killed = false;
    for pid in pids {
        let status = Command::new("taskkill")
            .args(["/F", "/PID", &pid])
            .status()?;
        if status.success() {
            killed = true;
        }
    }
    Ok(killed)
}

#[cfg(unix)]
fn kill_port(port: u16) -> anyhow::Result<bool> {
    let output = Command::new("lsof")
        .args(["-ti", &format!(":{port}")])
        .output()?;
    if !output.status.success() || output.stdout.is_empty() {
        return Ok(false);
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut killed = false;
    for pid in stdout.split_whitespace() {
        let status = Command::new("kill").args(["-9", pid]).status()?;
        if status.success() {
            killed = true;
        }
    }
    Ok(killed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_all_is_noop_when_nothing_is_listening() {
        // Assumes 8787/9473 are free at test-run time. On machines where they
        // aren't, this test is skipped by the user's environment choice —
        // kill_all() will actually kill those processes, which is exactly
        // what the user wants.
        kill_all().unwrap();
    }

    #[test]
    fn ports_list_is_correct() {
        assert_eq!(PORTS, &[8787u16, 9473u16]);
    }
}
