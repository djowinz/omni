//! Append-only local audit log at `~/.omni-admin/audit.log`.
//!
//! Every state-changing admin action (REMOVE, BAN, UNBAN, VOCAB, LIMITS,
//! ACTION) appends one timestamped line here. The file is forensic-only —
//! the Worker has no read path. Authoritative records of moderator actions
//! live in Worker KV/D1 state + Worker access logs; this log exists so the
//! operator can answer "what did I do yesterday?" without round-tripping.

use std::io::Write;
use std::path::PathBuf;

/// Resolve `~/.omni-admin/audit.log`, creating the parent directory on demand.
pub fn log_path() -> anyhow::Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory for audit log"))?;
    let dir = base.home_dir().join(".omni-admin");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("audit.log"))
}

/// Append a line to the audit log, prefixed with an RFC 3339 UTC timestamp.
///
/// Callers pass the semantic body (e.g. `REMOVE artifact=... reason="..."`);
/// the timestamp is prepended here so every line is uniformly shaped.
pub fn append(line: &str) -> anyhow::Result<()> {
    let path = log_path()?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{} {}", chrono::Utc::now().to_rfc3339(), line)?;
    Ok(())
}
