//! End-to-end flow tests (Task 21 of #012).
//!
//! The Wave D per-command test binaries (`reports_cli.rs`, `pubkey_cli.rs`,
//! `limits_cli.rs`, ...) each mount ONE endpoint and drive ONE subcommand.
//! This binary composes multi-endpoint / multi-invocation flows that
//! exercise realistic moderator workflows against a single `wiremock`
//! server, catching regressions that only surface across calls — e.g.
//! cascade-audit line accumulation or admin-error exit plumbing.
//!
//! Layer A of the T21 plan; Layer B lives worker-side.
//!
//! Deliberate omissions (documented to avoid re-litigation):
//!
//!   * The interactive `reports review` loop is NOT exercised end-to-end
//!     here — driving `dialoguer::Select` requires a PTY and
//!     `assert_cmd::Command` doesn't spawn one. `reports_action_twice_*`
//!     below covers the ACTION endpoint the loop delegates to; the
//!     dialoguer prompt itself is covered at the unit level.
//!
//!   * `--force` header propagation is already tested in
//!     `limits_cli::limits_set_force_sends_header` — not duplicated.
//!
//!   * Spec §10's "wrangler dev" dream is intentionally NOT implemented;
//!     see the T21 plan delta. Wiremock + the live Miniflare suites in
//!     `services/omni-themes-worker/test/` together give equivalent
//!     coverage without the flake / Node dep.

use assert_cmd::Command;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;
use common::mint_key;

/// Admin-kind error body → non-zero CLI exit + stderr carries the kind/detail.
///
/// Worker returns 403 with `{"error":{"kind":"Admin","detail":"NotModerator",...}}`
/// when the signer isn't on the moderator list. The CLI can't know that locally
/// (every mint_key call produces a valid Ed25519 JWS), so we simulate by making
/// the mock itself return the not-moderator envelope. This validates the CLI's
/// error-propagation pipeline: the JSON envelope is parsed, non-success status
/// flips to an `Err(AdminError::Response)`, and anyhow prints the kind/detail.
#[tokio::test(flavor = "multi_thread")]
async fn admin_not_moderator_envelope_surfaces_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/admin/stats"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "error": {
                "code": "ADMIN_NOT_MODERATOR",
                "kind": "Admin",
                "detail": "NotModerator",
                "message": "signer is not on OMNI_ADMIN_PUBKEYS"
            }
        })))
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
        .args(["stats"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "expected non-zero exit for Admin.NotModerator, got success. stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Both the kind and the detail travel through anyhow's Display chain
    // (`client::AdminError` prints `HTTP {status}: {body}` with the full
    // envelope body attached).
    assert!(
        stderr.contains("Admin") && stderr.contains("NotModerator"),
        "stderr missing kind/detail: {stderr}"
    );
    // Spec §6: Admin-kind errors must exit with code 2 specifically, not
    // just "non-zero". Anything else means the dispatcher's downcast +
    // `kind_to_exit_code` pipeline regressed.
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit code 2 for Admin kind, got {:?}",
        output.status.code()
    );
}

/// Auth-kind envelope (e.g. BadSignature) must surface as exit code 3.
///
/// Guards spec §6: `Admin=2, Auth=3, Malformed/Integrity=4, Io=5, Quota=6`.
/// Previously the CLI coalesced everything into anyhow → exit 1; this test
/// pins the `kind_to_exit_code` dispatch path.
#[tokio::test(flavor = "multi_thread")]
async fn auth_envelope_maps_to_exit_code_3() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/admin/stats"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "error": {
                "code": "AUTH_BAD_SIGNATURE",
                "kind": "Auth",
                "detail": "BadSignature",
                "message": "signature verification failed"
            }
        })))
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
        .args(["stats"])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(3),
        "expected exit code 3 for Auth kind, got {:?}. stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Two sequential `pubkey ban` invocations → two independent audit lines,
/// each carrying the response's `cascade_count`.
///
/// First call mounts a cascade_count=3 response (fresh ban), second a
/// cascade_count=0 response (already banned). Confirms the audit log is
/// append-only and that cascade metadata is faithfully recorded per call
/// — this is what operators rely on to prove idempotence after the fact.
#[tokio::test(flavor = "multi_thread")]
async fn ban_author_cascade_audit_accumulates_across_invocations() {
    let server = MockServer::start().await;
    // First call → cascade_count=3. `up_to_n_times(1)` limits this mock to a
    // single match so the second call falls through to the fresh-mock below.
    Mock::given(method("POST"))
        .and(path("/v1/admin/pubkey/ban"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pubkey": "aa",
            "cascade_count": 3,
            "cascade_errors": 0
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    // Second call → cascade_count=0 (idempotent rerun).
    Mock::given(method("POST"))
        .and(path("/v1/admin/pubkey/ban"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "pubkey": "aa",
            "cascade_count": 0,
            "cascade_errors": 0
        })))
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    let audit_dir = tmp.path().join(".omni-admin");

    for _ in 0..2 {
        let output = Command::cargo_bin("admin")
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
    }

    let log = audit_dir.join("audit.log");
    let contents = std::fs::read_to_string(&log).unwrap();
    let ban_lines: Vec<&str> = contents
        .lines()
        .filter(|l| l.contains("BAN pubkey=aa"))
        .collect();
    assert_eq!(
        ban_lines.len(),
        2,
        "expected exactly 2 BAN audit lines, got {}: {contents}",
        ban_lines.len()
    );
    assert!(
        ban_lines.iter().any(|l| l.contains("cascade_count=3")),
        "no line with cascade_count=3: {contents}"
    );
    assert!(
        ban_lines.iter().any(|l| l.contains("cascade_count=0")),
        "no line with cascade_count=0: {contents}"
    );
}

/// Two sequential `reports action` invocations on distinct report ids →
/// two `ACTION report=<id>` audit lines.
///
/// This is the `review` interactive-loop contract reduced to its audited
/// side effect: each approval in the TUI ultimately POSTs
/// `/v1/admin/report/:id/action`, which appends one line. Covering two ids
/// proves the per-id path-regex mock + audit plumbing wire up correctly
/// across calls, without a PTY. See the module-doc omissions note.
#[tokio::test(flavor = "multi_thread")]
async fn reports_action_twice_covers_review_loop_endpoints() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"^/v1/admin/report/[^/]+/action$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "ok"
        })))
        .mount(&server)
        .await;
    let tmp = tempfile::TempDir::new().unwrap();
    let key = mint_key(tmp.path());
    let audit_dir = tmp.path().join(".omni-admin");

    for id in ["report-one", "report-two"] {
        let output = Command::cargo_bin("admin")
            .unwrap()
            .env("OMNI_ADMIN_AUDIT_DIR", &audit_dir)
            .args(["--worker-url"])
            .arg(server.uri())
            .args(["--key-file"])
            .arg(&key)
            .args(["reports", "action", id, "--action", "no-action"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "id={id} stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let log = audit_dir.join("audit.log");
    let contents = std::fs::read_to_string(&log).unwrap();
    assert!(
        contents.contains("ACTION report=report-one"),
        "missing report-one line: {contents}"
    );
    assert!(
        contents.contains("ACTION report=report-two"),
        "missing report-two line: {contents}"
    );
}
