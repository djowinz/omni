//! Integration tests for `omni-admin review` + `omni-admin stats`.
//!
//! `stats` is a trivial read-only roundtrip — fully covered here against
//! a `wiremock` server. The `review` loop is driven by `dialoguer`, which
//! requires a real TTY; we only assert here that `review --help` wires up,
//! and defer end-to-end coverage of the interactive loop to T21.

use assert_cmd::Command;
use wiremock::matchers::{method, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::mint_key;

#[tokio::test(flavor = "multi_thread")]
async fn stats_emits_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/v1/admin/stats$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pending_reports": 2,
            "banned_pubkeys": 1,
            "banned_devices": 0,
            "total_artifacts": 5,
            "total_installs": 42,
            "vocab_version": 3,
            "limits_version": 2,
        })))
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    let out = Command::cargo_bin("admin")
        .unwrap()
        .args(["--json", "--worker-url"])
        .arg(server.uri())
        .args(["--key-file"])
        .arg(&key)
        .args(["stats"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout not JSON ({e}): {stdout}"));
    assert_eq!(v["pending_reports"], 2);
    assert_eq!(v["banned_pubkeys"], 1);
}

#[test]
fn review_help_prints_usage() {
    Command::cargo_bin("admin")
        .unwrap()
        .args(["review", "--help"])
        .assert()
        .success();
}
