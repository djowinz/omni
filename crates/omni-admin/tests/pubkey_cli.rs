//! Integration tests for `omni-admin pubkey {ban, unban}`.
//!
//! Mirrors `artifact_cli.rs`: wiremock server, fresh keygen, assertion on
//! both HTTP shape and audit-log side effects. `--yes` is passed where
//! needed to bypass the interactive cascade confirmation.

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
async fn pubkey_ban_appends_audit_with_cascade_counts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/admin/pubkey/ban"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pubkey": "aa",
            "cascade_count": 3,
            "cascade_errors": 0
        })))
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    let audit_dir = tmp.path().join(".omni-admin");
    let output = Command::cargo_bin("omni-admin")
        .unwrap()
        .env("OMNI_ADMIN_AUDIT_DIR", &audit_dir)
        .args(["--yes", "--worker-url"])
        .arg(server.uri())
        .args(["--key-file"])
        .arg(&key)
        .args(["pubkey", "ban", "aa", "--reason", "spam"])
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
        contents.contains("BAN pubkey=aa"),
        "log contents: {contents}"
    );
    assert!(
        contents.contains("cascade_count=3"),
        "log contents: {contents}"
    );
    assert!(
        contents.contains("cascade_errors=0"),
        "log contents: {contents}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pubkey_unban_appends_audit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/admin/pubkey/unban"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"pubkey":"aa","status":"unbanned"})),
        )
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
        .args(["pubkey", "unban", "aa"])
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
        contents.contains("UNBAN pubkey=aa"),
        "log contents: {contents}"
    );
}
