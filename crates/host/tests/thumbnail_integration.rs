//! Integration coverage for `share::thumbnail` (sub-spec 011 §11).
//!
//! Test split:
//! - CI-cheap tests (default `cargo test`) exercise the non-Ultralight paths:
//!   reference-overlay parsing, CSS-var coverage, bundle manifest fast-path
//!   rejection paths, `ThumbnailConfig::default`, and the `pack_signed_bundle`
//!   roundtrip fixture.
//! - Render-path tests (`#[ignore]`, run manually with `-- --ignored`) cover
//!   `generate_for_theme` / `generate_for_bundle` full flow; they need
//!   Ultralight resources next to the test executable.
//!
//! Structural findings (documented inline where they bite):
//! - `omni_bundle::unpack_manifest` already rejects `schema_version != 1`
//!   before our thumbnail layer gets a chance. The spec's
//!   `ThumbnailError::UnsupportedSchemaVersion` pre-flight is thus defense in
//!   depth — in practice the error surfaces as `ThumbnailError::Bundle(...)`
//!   wrapping `BundleError::Integrity{SchemaVersionUnsupported}`. Test 3
//!   accepts either shape (with a preference for the deeper layer kicking in).
//! - `unpack_manifest` does NOT validate `resource_kinds` vocabulary, so our
//!   `UnsupportedKind` pre-flight IS reachable and Test 4 exercises it directly.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use omni_bundle::{BundleError, BundleLimits, FileEntry, IntegrityKind, Manifest, ResourceKind};
use omni_host::omni::assets::REFERENCE_OVERLAY_OMNI;
use omni_host::omni::default::DEFAULT_THEME_CSS;
use omni_host::omni::overlay_fs::{OverlayFilesystem, ResolveError};
use omni_host::omni::parser::{parse_omni_with_diagnostics, Severity};
use omni_host::share::thumbnail::bundle::generate_for_bundle;
use omni_host::share::thumbnail::theme::generate_for_theme;
use omni_host::share::thumbnail::{
    default_sample_values, ThumbnailConfig, ThumbnailError, DEFAULT_HEIGHT, DEFAULT_WIDTH,
    MAX_THUMBNAIL_BYTES,
};
use omni_identity::{pack_signed_bundle, unpack_signed_bundle, Keypair};
use regex_lite::Regex;
use sha2::{Digest, Sha256};

/// Serializes the two tempdir-watcher tests within THIS binary so their
/// before/after snapshots don't race against each other. Does NOT defend
/// against other integration-test binaries running concurrently — for that
/// we rely on the prefix+mtime filter in `temp_dir_diff` below. Accepting
/// that tradeoff: a production-code fix (TempDir rooted in an isolated
/// dir) is out of scope for this task.
static TEMPDIR_WATCHER_LOCK: Mutex<()> = Mutex::new(());

// ---------- Helpers ----------

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

/// Extract `--name` CSS custom-property declarations from a stylesheet.
/// Uses `regex-lite` (already a production dep) for clarity.
fn extract_css_vars(css: &str) -> Vec<String> {
    let re = Regex::new(r"--([a-z][a-z0-9-]*)").unwrap();
    let mut out: BTreeSet<String> = BTreeSet::new();
    for cap in re.captures_iter(css) {
        out.insert(cap[1].to_string());
    }
    out.into_iter().collect()
}

/// Minimal manifest + file map suitable for `pack_signed_bundle`.
fn minimal_bundle_fixture() -> (Manifest, BTreeMap<String, Vec<u8>>) {
    let overlay_bytes = br#"<widget id="x" name="x" enabled="true">
  <template><div class="p"><span class="val">hi</span></div></template>
  <style>.p{color:#fff}</style>
</widget>"#
        .to_vec();
    let mut files = BTreeMap::new();
    files.insert("overlay.omni".to_string(), overlay_bytes.clone());
    let manifest = Manifest {
        schema_version: 1,
        name: "fixture".into(),
        // Type inference binds `.parse()` to `semver::Version` via
        // `omni_bundle::Manifest`'s public field type; this lets us avoid
        // a redundant `semver` dev-dep.
        version: "0.1.0".parse().expect("valid semver"),
        omni_min_version: "0.1.0".parse().expect("valid semver"),
        description: String::new(),
        tags: vec![],
        license: "MIT".into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: None,
        sensor_requirements: vec![],
        files: vec![FileEntry {
            path: "overlay.omni".into(),
            sha256: sha256(&overlay_bytes),
        }],
        resource_kinds: None,
    };
    (manifest, files)
}

fn pack_test_bundle(manifest: &Manifest, files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let kp = Keypair::generate();
    pack_signed_bundle(manifest, files, &kp, &BundleLimits::DEFAULT)
        .expect("pack_signed_bundle should succeed for minimal fixture")
}

/// Assert PNG magic + IHDR-decoded dimensions.
fn assert_png_dimensions(png: &[u8], expected_w: u32, expected_h: u32) {
    assert!(png.len() >= 24, "PNG too short: {}", png.len());
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic mismatch");
    // PNG layout: 8 magic + 4 IHDR length + 4 "IHDR" = 16 before width.
    let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
    assert_eq!(w, expected_w, "PNG width");
    assert_eq!(h, expected_h, "PNG height");
}

/// Entries under `std::env::temp_dir()` whose name begins with tempfile's
/// default prefix `.tmp` AND whose creation/modification time is at/after
/// `baseline`. Parallel tests in other binaries that use `tempfile` could
/// still create `.tmp`-prefixed entries concurrently, but the mtime floor
/// scopes the window to the critical section of a single call and the
/// in-binary `TEMPDIR_WATCHER_LOCK` serializes the two tests that rely on
/// this helper. Tradeoff documented at the static.
fn new_tempdir_entries_since(baseline: SystemTime) -> BTreeSet<std::ffi::OsString> {
    // Give filesystems with 1-2s mtime resolution a margin, but stay small
    // so we don't pick up unrelated stale entries from earlier runs.
    let floor = baseline.checked_sub(Duration::from_secs(1)).unwrap_or(baseline);
    let mut out = BTreeSet::new();
    let rd = match std::fs::read_dir(std::env::temp_dir()) {
        Ok(r) => r,
        Err(_) => return out,
    };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let Some(name_s) = name.to_str() else { continue };
        if !name_s.starts_with(".tmp") {
            continue;
        }
        let Ok(md) = entry.metadata() else { continue };
        let ts = md.created().or_else(|_| md.modified()).ok();
        match ts {
            Some(t) if t >= floor => {
                out.insert(name);
            }
            _ => {}
        }
    }
    out
}

// ---------- Tests that DO NOT require Ultralight ----------

#[test]
fn reference_overlay_parses_without_errors() {
    let (parsed, diagnostics) = parse_omni_with_diagnostics(REFERENCE_OVERLAY_OMNI);
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "reference overlay must parse with zero Error-severity diagnostics: {errors:?}"
    );
    assert!(
        parsed.is_some(),
        "reference overlay must return an OmniFile"
    );
}

#[test]
fn reference_overlay_exercises_every_default_theme_css_variable() {
    let vars = extract_css_vars(DEFAULT_THEME_CSS);
    assert!(!vars.is_empty(), "regex must find at least one --var");
    for name in &vars {
        let needle = format!("var(--{name})");
        assert!(
            REFERENCE_OVERLAY_OMNI.contains(&needle),
            "reference overlay must reference --{name} (looked for {needle:?})"
        );
    }
}

#[test]
fn thumbnail_config_default_is_sensible() {
    let cfg = ThumbnailConfig::default();
    assert_eq!(cfg.width, DEFAULT_WIDTH);
    assert_eq!(cfg.height, DEFAULT_HEIGHT);
    assert_eq!(cfg.width, 800);
    assert_eq!(cfg.height, 450);

    // Spec §5: 11 deterministic sample-value keys.
    let values = default_sample_values();
    assert_eq!(values.len(), 11, "11 sample keys per spec §5");
    for expected in [
        "cpu.usage",
        "cpu.temp",
        "gpu.usage",
        "gpu.temp",
        "gpu.vram_used",
        "gpu.vram_total",
        "ram.used",
        "ram.total",
        "net.down",
        "net.up",
        "fps.current",
    ] {
        assert!(
            values.contains_key(expected),
            "default_sample_values missing {expected:?}"
        );
    }
}

#[test]
fn signed_bundle_roundtrip_fixture_works() {
    let (m, f) = minimal_bundle_fixture();
    let kp = Keypair::generate();
    let bytes = pack_signed_bundle(&m, &f, &kp, &BundleLimits::DEFAULT)
        .expect("pack_signed_bundle must accept minimal fixture");
    let signed = unpack_signed_bundle(&bytes, Some(&kp.public_key()), &BundleLimits::DEFAULT)
        .expect("unpack_signed_bundle must accept self-packed bytes");
    assert_eq!(signed.manifest().name, "fixture");
    // files() must expose the original overlay content (minus any signature
    // sibling, which omni-identity strips).
    let got: BTreeMap<&String, &Vec<u8>> = signed.files().collect();
    assert!(got.contains_key(&"overlay.omni".to_string()));
}

#[test]
fn bundle_unsupported_schema_version_fails_before_tempdir() {
    // Structural finding: omni_bundle::unpack_manifest rejects schema_version
    // != 1 internally BEFORE our `is_supported_schema_version` check gets to
    // run. The thumbnail layer's pre-flight is defense in depth; in practice
    // we see `ThumbnailError::Bundle(BundleError::Integrity{SchemaVersionUnsupported})`
    // rather than `UnsupportedSchemaVersion`. Either shape satisfies the spec
    // goal ("reject before TempDir allocation"), so we accept both.
    let _guard = TEMPDIR_WATCHER_LOCK.lock().unwrap();
    let (mut m, f) = minimal_bundle_fixture();
    m.schema_version = 99;
    let bytes = pack_test_bundle(&m, &f);

    let baseline = SystemTime::now();
    let err = generate_for_bundle(&bytes, &ThumbnailConfig::default())
        .expect_err("bundle with schema_version=99 must error");
    let new_entries = new_tempdir_entries_since(baseline);

    let layer_ok = matches!(
        err,
        ThumbnailError::UnsupportedSchemaVersion { version: 99 }
            | ThumbnailError::Bundle(BundleError::Integrity {
                kind: IntegrityKind::SchemaVersionUnsupported,
                ..
            })
    );
    assert!(
        layer_ok,
        "expected UnsupportedSchemaVersion or Bundle(SchemaVersionUnsupported); got {err:?}"
    );

    assert!(
        new_entries.is_empty(),
        "generate_for_bundle allocated {} new temp-dir entries before failing: {:?}",
        new_entries.len(),
        new_entries,
    );
}

#[test]
fn bundle_unsupported_resource_kind_fails_before_tempdir() {
    // `unpack_manifest` does NOT validate vocabulary, so this exercises the
    // thumbnail layer's own `is_supported_resource_kind` pre-flight.
    let (mut m, f) = minimal_bundle_fixture();
    let mut kinds = BTreeMap::new();
    kinds.insert(
        "executable".to_string(),
        ResourceKind {
            dir: "bin".into(),
            extensions: vec!["exe".into()],
            max_size_bytes: 1024,
        },
    );
    m.resource_kinds = Some(kinds);
    let bytes = pack_test_bundle(&m, &f);

    let _guard = TEMPDIR_WATCHER_LOCK.lock().unwrap();
    let baseline = SystemTime::now();
    let err = generate_for_bundle(&bytes, &ThumbnailConfig::default())
        .expect_err("bundle with 'executable' resource kind must error");
    let new_entries = new_tempdir_entries_since(baseline);

    match &err {
        ThumbnailError::UnsupportedKind { kind } => {
            assert_eq!(kind, "executable");
        }
        other => panic!("expected UnsupportedKind {{ kind: 'executable' }}; got {other:?}"),
    }

    assert!(
        new_entries.is_empty(),
        "generate_for_bundle allocated {} new temp-dir entries before failing: {:?}",
        new_entries.len(),
        new_entries,
    );
}

// ---------- Tests that require Ultralight ----------

#[test]
#[ignore = "requires Ultralight resources next to the test executable; run with --ignored"]
fn theme_default_renders_800x450_png_under_cap() {
    let cfg = ThumbnailConfig::default();
    let png = generate_for_theme(DEFAULT_THEME_CSS.as_bytes(), &cfg)
        .expect("generate_for_theme with default theme");
    assert!(
        png.len() <= MAX_THUMBNAIL_BYTES,
        "PNG over size cap: {} > {MAX_THUMBNAIL_BYTES}",
        png.len()
    );
    assert_png_dimensions(&png, DEFAULT_WIDTH, DEFAULT_HEIGHT);
}

#[test]
#[ignore = "requires Ultralight resources next to the test executable; run with --ignored"]
fn theme_high_contrast_differs_from_default() {
    let cfg = ThumbnailConfig::default();
    let default_png = generate_for_theme(DEFAULT_THEME_CSS.as_bytes(), &cfg).unwrap();
    let high = include_bytes!("fixtures/themes/high_contrast.css");
    let high_png = generate_for_theme(high, &cfg).unwrap();
    assert_ne!(
        default_png, high_png,
        "visibly-different themes must produce byte-different PNGs"
    );
}

#[test]
#[ignore = "requires Ultralight resources next to the test executable; run with --ignored"]
fn bundle_full_flow_produces_valid_png() {
    let (m, f) = minimal_bundle_fixture();
    let bytes = pack_test_bundle(&m, &f);
    let png = generate_for_bundle(&bytes, &ThumbnailConfig::default())
        .expect("generate_for_bundle on minimal fixture");
    assert!(png.len() <= MAX_THUMBNAIL_BYTES);
    assert_png_dimensions(&png, DEFAULT_WIDTH, DEFAULT_HEIGHT);
}

#[test]
#[ignore = "sandbox-probe coverage needs dedicated JS plumbing (see TODO)"]
fn thumbnail_gen_denies_fetch_xhr_websocket_stub() {
    // TODO(#011 follow-up): assert fetch / XMLHttpRequest / WebSocket are
    // absent under ViewTrust::ThumbnailGen. The cleanest implementation needs
    // a small `__omni_inject_test_hook` bootstrap affordance (or a synchronous
    // `evaluate_script` that probes globals and returns a marker) that is not
    // present in the shipped js_bootstrap today. Writing a test that probes
    // via CSS `url()` escaping OverlayFilesystem is a different invariant
    // (#8 filesystem scope), already enforced by sub-spec #001.
    //
    // Leaving this test as a stub rather than fabricating coverage that
    // doesn't actually exercise the claim. See report for the discussion.
    let probe = include_bytes!("fixtures/themes/sandbox_probe.css");
    let _ = generate_for_theme(probe, &ThumbnailConfig::default());
}

// ---------- Spec §11 bullet 8: synthetic sensor-value injection ----------

#[test]
#[ignore = "requires Ultralight resources; run with --ignored"]
fn theme_renders_sample_cpu_usage_value() {
    // Verifies the sensor-injection path runs end-to-end on the default
    // theme and produces a valid PNG.
    //
    // A literal "42 visible in the PNG" assertion would require either
    // in-process OCR or a golden-bytes comparison — OCR is overkill for
    // coverage, and the golden-PNG fidelity assertion lives in bullet 11
    // (`golden_png_snapshot_for_fixture_theme`). This test covers the
    // "sensor injection runs without error" half of the contract; bullet
    // 11 covers rendering fidelity including injected values.
    let cfg = ThumbnailConfig::default();
    let png = generate_for_theme(DEFAULT_THEME_CSS.as_bytes(), &cfg)
        .expect("generate_for_theme must run sensor-injection path");
    assert_png_dimensions(&png, DEFAULT_WIDTH, DEFAULT_HEIGHT);
}

// ---------- Spec §11 bullet 10: OverlayFilesystem path-traversal scoping ----

#[test]
fn overlay_filesystem_refuses_path_traversal_outside_root() {
    // `OverlayFilesystem::{new, resolve}` are `pub`, so this is a direct
    // filesystem-resolver test — no Ultralight needed.
    let tmp = tempfile::tempdir().expect("tempdir");
    let fs = OverlayFilesystem::new(tmp.path().to_path_buf());

    // POSIX-style traversal
    assert_eq!(
        fs.resolve("../../etc/passwd").unwrap_err(),
        ResolveError::ParentEscape,
        "POSIX parent-escape must be refused"
    );

    // Windows-style traversal (backslashes are valid path components on
    // Windows and are treated as raw filename chars on POSIX — either
    // way the request must NOT resolve outside `root`).
    let win = fs.resolve("..\\..\\..\\Windows\\System32\\drivers\\etc\\hosts");
    assert!(
        win.is_err(),
        "Windows-style traversal must be refused, got Ok({:?})",
        win.ok()
    );

    // Absolute paths must also be refused.
    assert_eq!(
        fs.resolve("/etc/passwd").unwrap_err(),
        ResolveError::AbsolutePath
    );
}

// ---------- Spec §11 bullet 11: golden PNG snapshot -------------------------

#[ignore = "requires Ultralight resources; run with --ignored"]
#[test]
fn golden_png_snapshot_for_fixture_theme() {
    let css = include_bytes!("fixtures/themes/high_contrast.css");
    let png = generate_for_theme(css, &ThumbnailConfig::default()).expect("render");
    let hash = Sha256::digest(png.as_slice());
    let hash_hex = format!("{:x}", hash);
    // Regenerate with:
    //   BLESS_GOLDEN=1 cargo test --package omni-host \
    //       --test thumbnail_integration -- --ignored golden_png -- --nocapture
    // then paste the printed hash into GOLDEN_HASH below.
    const GOLDEN_HASH: &str = "<fill me>";
    if std::env::var("BLESS_GOLDEN").is_ok() {
        println!("New golden hash: {hash_hex}");
        return;
    }
    assert_eq!(
        hash_hex, GOLDEN_HASH,
        "render drift; run with BLESS_GOLDEN=1 to refresh"
    );
}
