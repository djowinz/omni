//! Integration tests for `omni-admin limits {get, set}`.
//!
//! Mirrors `vocab_cli.rs`: wiremock server, fresh keygen, assertion on
//! stdout (get) and audit-log side effects (set). The `--force` test
//! mounts a single Mock gated on the `X-Omni-Admin-Force: true` header —
//! if the CLI omits the header the mock misses and wiremock returns 404.

use assert_cmd::Command;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::mint_key;

#[tokio::test(flavor = "multi_thread")]
async fn limits_get_emits_json() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/config/limits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "max_bundle_compressed": 4_194_304,
            "max_bundle_uncompressed": 16_777_216,
            "max_entries": 1000,
            "version": 2
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
        .args(["limits", "get"])
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
    assert_eq!(v.get("max_entries").and_then(|n| n.as_u64()), Some(1000));
}

#[tokio::test(flavor = "multi_thread")]
async fn limits_set_appends_audit() {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path("/v1/admin/limits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "max_bundle_compressed": 5_242_880,
            "version": 3
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
        .args(["limits", "set", "--max-bundle-compressed", "5242880"])
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
        contents.contains("LIMITS max_bundle_compressed=5242880 version_after=3"),
        "log contents: {contents}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn limits_set_force_sends_header() {
    let server = MockServer::start().await;
    // Only this mock is mounted; it requires the force header. If the
    // CLI omits it the request misses and wiremock returns 404, failing
    // the test.
    Mock::given(method("PATCH"))
        .and(path("/v1/admin/limits"))
        .and(header("X-Omni-Admin-Force", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "max_entries": 500,
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
        .args(["limits", "set", "--max-entries", "500", "--force"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
