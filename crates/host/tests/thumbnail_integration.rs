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

use omni_bundle::{BundleError, BundleLimits, FileEntry, IntegrityKind, Manifest, ResourceKind};
use omni_host::omni::assets::REFERENCE_OVERLAY_OMNI;
use omni_host::omni::default::DEFAULT_THEME_CSS;
use omni_host::omni::parser::{parse_omni_with_diagnostics, Severity};
use omni_host::share::thumbnail::bundle::generate_for_bundle;
use omni_host::share::thumbnail::theme::generate_for_theme;
use omni_host::share::thumbnail::{
    default_sample_values, ThumbnailConfig, ThumbnailError, DEFAULT_HEIGHT, DEFAULT_WIDTH,
    MAX_THUMBNAIL_BYTES,
};
use omni_identity::{pack_signed_bundle, unpack_signed_bundle, Keypair};
use sha2::{Digest, Sha256};

// ---------- Helpers ----------

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

/// Extract `--name` CSS custom-property declarations from a stylesheet.
///
/// Uses a lightweight manual scan (rather than the workspace `regex-lite`
/// dep, which is a production dep here) so test coverage does not add an
/// additional dev-dep for a trivial parse.
fn extract_css_vars(css: &str) -> Vec<String> {
    let mut out = BTreeSet::new();
    let bytes = css.as_bytes();
    let mut i = 0;
    while i + 2 < bytes.len() {
        if bytes[i] == b'-' && bytes[i + 1] == b'-' {
            let start = i + 2;
            let mut j = start;
            while j < bytes.len() {
                let c = bytes[j];
                let ok = c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-';
                if !ok {
                    break;
                }
                j += 1;
            }
            // Require leading lowercase letter, as specified.
            if j > start && bytes[start].is_ascii_lowercase() {
                out.insert(std::str::from_utf8(&bytes[start..j]).unwrap().to_string());
            }
            i = j.max(start + 1);
        } else {
            i += 1;
        }
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
        version: semver::Version::new(0, 1, 0),
        omni_min_version: semver::Version::new(0, 1, 0),
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

/// Snapshot of immediate entries under `std::env::temp_dir()`. The caller
/// compares snapshots before/after a closure to detect new tempdirs. We record
/// entry names (not inodes) because `TempDir::new` uses a unique random
/// filename under the system temp dir — any new name is evidence of
/// allocation.
fn temp_dir_snapshot() -> BTreeSet<std::ffi::OsString> {
    let mut out = BTreeSet::new();
    if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
        for entry in rd.flatten() {
            out.insert(entry.file_name());
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
    let (mut m, f) = minimal_bundle_fixture();
    m.schema_version = 99;
    let bytes = pack_test_bundle(&m, &f);

    let before = temp_dir_snapshot();
    let err = generate_for_bundle(&bytes, &ThumbnailConfig::default())
        .err()
        .expect("bundle with schema_version=99 must error");
    let after = temp_dir_snapshot();

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

    // No new entry under the system temp dir.
    let new_entries: BTreeSet<_> = after.difference(&before).collect();
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

    let before = temp_dir_snapshot();
    let err = generate_for_bundle(&bytes, &ThumbnailConfig::default())
        .err()
        .expect("bundle with 'executable' resource kind must error");
    let after = temp_dir_snapshot();

    match &err {
        ThumbnailError::UnsupportedKind { kind } => {
            assert_eq!(kind, "executable");
        }
        other => panic!("expected UnsupportedKind {{ kind: 'executable' }}; got {other:?}"),
    }

    let new_entries: BTreeSet<_> = after.difference(&before).collect();
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
