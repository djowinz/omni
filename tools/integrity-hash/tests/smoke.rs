//! Smoke test: when pointed at a real PE (this crate's own test binary is
//! not one, but any cargo-built .exe on Windows is), we get a 64-char hex
//! digest. For portability, assert the function-level behavior via a
//! synthetic minimal PE isn't worth the complexity; instead, test the CLI's
//! argument handling and that run() errors cleanly on bogus input.

use std::process::Command;

#[test]
fn cli_usage_when_no_args() {
    let exe = env!("CARGO_BIN_EXE_integrity-hash");
    let out = Command::new(exe).output().expect("run");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("usage:"));
}

#[test]
fn cli_errors_on_missing_file() {
    let exe = env!("CARGO_BIN_EXE_integrity-hash");
    let out = Command::new(exe)
        .arg("nonexistent-path-xyz.exe")
        .output()
        .expect("run");
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("nonexistent-path-xyz.exe"));
}
