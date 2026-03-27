use std::fs::OpenOptions;
use std::io::Write;

/// Log a message to a file in the temp directory. Intentionally simple —
/// no external dependencies. Will be replaced with structured logging later.
pub fn log_to_file(msg: &str) {
    let path = std::env::temp_dir().join("omni_overlay.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "[{timestamp}] {msg}");
    }
}
