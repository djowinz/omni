//! Integration tests for `omni-admin reports {list, show, action}`.
//!
//! Stands up a `wiremock` server, mints a fresh admin key via the `keygen`
//! subcommand, then drives the binary with `--worker-url <mock>` and asserts
//! both the HTTP shape (mock matchers) and the user-visible side effects
//! (JSON stdout / audit-log line).

use assert_cmd::Command;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::mint_key;

async fn start_list_mock(response_body: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/v1/admin/reports"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
        .mount(&server)
        .await;
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_list_emits_json() {
    let server = start_list_mock(serde_json::json!({ "items": [], "next_cursor": null })).await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    let output = Command::cargo_bin("admin")
        .unwrap()
        .args(["--json", "--worker-url"])
        .arg(server.uri())
        .args(["--key-file"])
        .arg(&key)
        .args(["reports", "list"])
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
    assert!(v.get("items").is_some(), "response missing `items`: {v}");
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_action_appends_audit() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/admin/report/[^/]+/action$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status":"ok"})))
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    // Redirect the audit log into tmp so we can assert on its contents
    // without clobbering the developer's real `~/.omni-admin/audit.log`.
    // `OMNI_ADMIN_AUDIT_DIR` is the test-only override documented on
    // `audit::log_path`; it bypasses `directories::BaseDirs`, which does
    // not honor a re-exported `USERPROFILE` on Windows.
    let audit_dir = tmp.path().join(".omni-admin");
    let output = Command::cargo_bin("admin")
        .unwrap()
        .env("OMNI_ADMIN_AUDIT_DIR", &audit_dir)
        .args(["--worker-url"])
        .arg(server.uri())
        .args(["--key-file"])
        .arg(&key)
        .args([
            "reports",
            "action",
            "report-xyz",
            "--action",
            "removed",
            "--notes",
            "spam",
        ])
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
        contents.contains("ACTION report=report-xyz"),
        "log contents: {contents}"
    );
    assert!(contents.contains("Removed"), "log contents: {contents}");
    assert!(contents.contains("spam"), "log contents: {contents}");
}
