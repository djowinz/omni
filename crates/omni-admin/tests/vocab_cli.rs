//! Integration tests for `omni-admin vocab {list, add, remove}`.
//!
//! Mirrors `pubkey_cli.rs`: wiremock server, fresh keygen, assertion on
//! stdout (list) and audit-log side effects (add/remove).

use assert_cmd::Command;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::mint_key;

#[tokio::test(flavor = "multi_thread")]
async fn vocab_list_emits_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/config/vocab"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tags": ["dark", "light"],
            "version": 3
        })))
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    let output = Command::cargo_bin("omni-admin")
        .unwrap()
        .args(["--json", "--worker-url"])
        .arg(server.uri())
        .args(["--key-file"])
        .arg(&key)
        .args(["vocab", "list"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect(&format!("stdout not JSON: {stdout}"));
    assert!(v.get("tags").and_then(|t| t.as_array()).is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn vocab_add_appends_audit() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/v1/admin/vocab"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tags": ["dark", "light", "retrowave"],
            "version": 4
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
        .args(["vocab", "add", "retrowave"])
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
        contents.contains("VOCAB add=retrowave version_after=4"),
        "log contents: {contents}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn vocab_remove_appends_audit() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/v1/admin/vocab"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "tags": ["dark"],
            "version": 5
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
        .args(["vocab", "remove", "light"])
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
        contents.contains("VOCAB remove=light version_after=5"),
        "log contents: {contents}"
    );
}
