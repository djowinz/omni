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
///
/// The `OMNI_ADMIN_AUDIT_DIR` env var overrides the default location. This
/// override exists so integration tests can redirect the log into a
/// per-test tempdir — on Windows `directories::BaseDirs` consults
/// `SHGetKnownFolderPath`, which does NOT honor a re-exported `USERPROFILE`,
/// so env-based home redirection is not portable.
pub fn log_path() -> anyhow::Result<PathBuf> {
    let dir = if let Some(p) = std::env::var_os("OMNI_ADMIN_AUDIT_DIR") {
        PathBuf::from(p)
    } else {
        let base = directories::BaseDirs::new()
            .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory for audit log"))?;
        base.home_dir().join(".omni-admin")
    };
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join("audit.log"))
}

/// Escape a value for the audit log's `key="value"` format.
///
/// Backslash and double-quote are escaped so a line with an operator-supplied
/// reason (e.g. `reason="they said \"hi\""`) round-trips through a naive
/// quote-aware parser. Newline + carriage-return are escaped to keep the
/// append-only "one event per line" invariant intact.
///
/// Only called from format-sites that wrap free-form operator input in
/// `"..."`; fixed-shape fields (ids, hex fingerprints, enum names) don't
/// need escaping and aren't routed through this helper.
pub fn escape_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_value_handles_quotes_and_backslashes() {
        assert_eq!(escape_value(r#"he said "hi""#), r#"he said \"hi\""#);
        assert_eq!(escape_value(r"C:\path"), r"C:\\path");
        assert_eq!(escape_value("a\nb"), "a\\nb");
        assert_eq!(escape_value("a\rb"), "a\\rb");
        assert_eq!(escape_value("plain text"), "plain text");
    }
}
