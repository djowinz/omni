//! Integration tests for `omni-admin device {ban, unban}`.
//!
//! Mirrors `pubkey_cli.rs` but against the device endpoints. No cascade,
//! so no interactive confirmation and no `--yes` required for ban.

use assert_cmd::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn mint_key(dir: &std::path::Path) -> std::path::PathBuf {
    let out = dir.join("admin-identity.key");
    Command::cargo_bin("omni-admin")
        .unwrap()
        .args(["keygen", "--output"])
        .arg(&out)
        .assert()
        .success();
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn device_ban_appends_audit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/admin/device/ban"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_fp": "deadbeef",
            "status": "banned"
        })))
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    let audit_dir = tmp.path().join(".omni-admin");
    let output = Command::cargo_bin("omni-admin")
        .unwrap()
        .env("OMNI_ADMIN_AUDIT_DIR", &audit_dir)
        .args(["--worker-url"])
        .arg(server.uri())
        .args(["--key-file"])
        .arg(&key)
        .args(["device", "ban", "deadbeef", "--reason", "abuse"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = audit_dir.join("audit.log");
    assert!(log.exists(), "audit log not found at {}", log.display());
    let contents = std::fs::read_to_string(&log).unwrap();
    assert!(
        contents.contains("BAN device=deadbeef"),
        "log contents: {contents}"
    );
    assert!(
        contents.contains("reason=\"abuse\""),
        "log contents: {contents}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn device_unban_appends_audit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/admin/device/unban"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"device_fp":"deadbeef","status":"unbanned"}),
        ))
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    let audit_dir = tmp.path().join(".omni-admin");
    let output = Command::cargo_bin("omni-admin")
        .unwrap()
        .env("OMNI_ADMIN_AUDIT_DIR", &audit_dir)
        .args(["--worker-url"])
        .arg(server.uri())
        .args(["--key-file"])
        .arg(&key)
        .args(["device", "unban", "deadbeef"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = audit_dir.join("audit.log");
    assert!(log.exists(), "audit log not found at {}", log.display());
    let contents = std::fs::read_to_string(&log).unwrap();
    assert!(
        contents.contains("UNBAN device=deadbeef"),
        "log contents: {contents}"
    );
}
