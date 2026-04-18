//! Integration tests for `omni-admin artifact {show, remove}`.
//!
//! Mirrors the `reports_cli.rs` pattern: wiremock server, fresh keygen,
//! assert both HTTP shape and user-visible side effects (JSON stdout /
//! audit-log line).

use assert_cmd::Command;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::mint_key;

#[tokio::test(flavor = "multi_thread")]
async fn artifact_show_emits_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/v1/artifact/[\w-]+$"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"abc","name":"t"})),
        )
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    let output = Command::cargo_bin("admin")
        .unwrap()
        .args(["--json", "--worker-url"])
        .arg(server.uri())
        .args(["--key-file"])
        .arg(&key)
        .args(["artifact", "show", "abc"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("stdout must be valid JSON");
    assert_eq!(v["id"], "abc");
}

#[tokio::test(flavor = "multi_thread")]
async fn artifact_remove_appends_audit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/admin/artifact/[\w-]+/remove$"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"artifact_id":"abc","status":"removed"})),
        )
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    // `OMNI_ADMIN_AUDIT_DIR` redirects the audit log into tmp; see
    // `audit::log_path` for why this is needed on Windows.
    let audit_dir = tmp.path().join(".omni-admin");
    let output = Command::cargo_bin("admin")
        .unwrap()
        .env("OMNI_ADMIN_AUDIT_DIR", &audit_dir)
        .args(["--worker-url"])
        .arg(server.uri())
        .args(["--key-file"])
        .arg(&key)
        .args(["artifact", "remove", "abc", "--reason", "copyright"])
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
        contents.contains("REMOVE artifact=abc"),
        "log contents: {contents}"
    );
    assert!(
        contents.contains("reason=\"copyright\""),
        "log contents: {contents}"
    );
}
