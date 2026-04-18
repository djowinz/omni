use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn keygen_produces_loadable_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = tmp.path().join("admin-identity.key");
    Command::cargo_bin("admin")
        .unwrap()
        .args(["keygen", "--output"])
        .arg(&out)
        .assert()
        .success()
        .stdout(predicate::str::contains("Admin pubkey"));
    assert!(out.exists());
    // loads cleanly
    let kp = omni_identity::Keypair::load_or_create(&out).expect("load round-trip");
    let _ = kp.public_key();
}

#[test]
fn keygen_refuses_to_overwrite_existing_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = tmp.path().join("admin-identity.key");
    std::fs::write(&out, b"existing").unwrap();
    Command::cargo_bin("admin")
        .unwrap()
        .args(["keygen", "--output"])
        .arg(&out)
        .assert()
        .failure()
        .stderr(predicate::str::contains("refusing to overwrite"));
}

#[test]
fn keygen_json_mode_emits_pubkey_hex_field() {
    let tmp = tempfile::TempDir::new().unwrap();
    let out = tmp.path().join("admin-identity.key");
    let output = Command::cargo_bin("admin")
        .unwrap()
        .args(["--json", "keygen", "--output"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v.get("pubkey_hex").and_then(|p| p.as_str()).is_some());
}
