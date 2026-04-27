//! Integration tests for `share::dep_resolver` (OWI-40 / Task A1.6 +
//! OWI-54 / Task B1.4).
//!
//! Spec: docs/superpowers/specs/2026-04-21-upload-flow-redesign-design.md §8.4
//! steps 1-7, INV-7.8.1, INV-7.8.2, INV-7.8.3, INV-7.8.4 (missing-refs +
//! unused-files + content-safety).
//!
//! These tests exercise the resolver against in-memory `BTreeMap` workspaces
//! (the same shape `walk_bundle` produces) so a real filesystem isn't needed.
//! Each test pins one INV from §7.8:
//!
//! - `resolves_inline_style_image_refs` — INV-7.8.1 image refs from inline
//!   `<style>` blocks; bundle order is `[overlay.omni, refs...]`.
//! - `flags_missing_image_ref` — INV-7.8.4 missing-refs category.
//! - `flags_unused_image_in_folder` — INV-7.8.4 unused-files category.
//! - `transitive_theme_walk_with_cycle_guard` — INV-7.8.2 transitive walk +
//!   INV-7.8.3 cycle guard (a `theme.css` whose body references itself does
//!   not infinite-loop).
//! - `flags_content_safety_violation_for_rejected_image` — INV-7.7.2 site
//!   #2 and INV-7.8.4 content-safety. Uses the test-only
//!   `resolve_with_moderation` entry point with an injected `Rejected`
//!   outcome so the assertion doesn't depend on a real NSFW fixture or a
//!   loaded ONNX model.
//! - `content_safety_skipped_outcome_does_not_violate` — INV-7.7.3 plus the
//!   `ModerationOutcome::Skipped` decay path (model not loaded). Pins the
//!   "no violation" contract so cargo test doesn't fail when the bundled
//!   ONNX model isn't present.

use omni_host::share::dep_resolver::{
    resolve, resolve_with_moderation, ModerationOutcome, Violation,
};
use std::collections::BTreeMap;

#[test]
fn resolves_inline_style_image_refs() {
    let mut workspace_files = BTreeMap::new();
    workspace_files.insert(
        "overlay.omni".into(),
        br#"<widget><template><div/></template><style>.x{background:url(images/bg.png)}</style></widget>"#.to_vec(),
    );
    workspace_files.insert("images/bg.png".into(), vec![0x89, 0x50, 0x4E, 0x47]);

    let result = resolve(&workspace_files).expect("resolve must succeed on valid workspace");
    assert_eq!(
        result.bundled_paths,
        vec!["overlay.omni".to_string(), "images/bg.png".to_string()],
        "overlay.omni first, then resolved refs in deterministic order"
    );
    assert!(
        result.violations.is_empty(),
        "no violations expected; got {:?}",
        result.violations
    );
}

#[test]
fn flags_missing_image_ref() {
    let mut workspace_files = BTreeMap::new();
    workspace_files.insert(
        "overlay.omni".into(),
        br#"<widget><template><div/></template><style>.x{background:url(images/missing.png)}</style></widget>"#.to_vec(),
    );

    let result = resolve(&workspace_files).expect("resolve must succeed even with missing refs");
    assert!(
        result.violations.iter().any(|v| matches!(
            v,
            Violation::MissingRef { path } if path == "images/missing.png"
        )),
        "expected MissingRef for images/missing.png; got {:?}",
        result.violations
    );
}

#[test]
fn flags_unused_image_in_folder() {
    let mut workspace_files = BTreeMap::new();
    workspace_files.insert(
        "overlay.omni".into(),
        br#"<widget><template><div/></template></widget>"#.to_vec(),
    );
    workspace_files.insert("images/orphan.png".into(), vec![0x89, 0x50, 0x4E, 0x47]);

    let result = resolve(&workspace_files).expect("resolve must succeed even with orphans");
    assert!(
        result.violations.iter().any(|v| matches!(
            v,
            Violation::UnusedFile { path } if path == "images/orphan.png"
        )),
        "expected UnusedFile for images/orphan.png; got {:?}",
        result.violations
    );
}

#[test]
fn flags_content_safety_violation_for_rejected_image() {
    // Workspace: overlay references one image; the injected moderator flags
    // the bundled image as Rejected with a deterministic confidence so we
    // can assert the violation shape without depending on a real NSFW
    // fixture or a loaded ONNX model.
    let mut workspace_files = BTreeMap::new();
    workspace_files.insert(
        "overlay.omni".into(),
        br#"<widget><template><div/></template><style>.x{background:url(images/sus.png)}</style></widget>"#.to_vec(),
    );
    workspace_files.insert(
        "images/sus.png".into(),
        // Minimal PNG signature; the moderator closure ignores bytes and
        // returns Rejected unconditionally so we don't need real image data.
        vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    );

    let moderator = |path: &str, _bytes: &[u8]| -> ModerationOutcome {
        if path == "images/sus.png" {
            ModerationOutcome::Rejected { unsafe_score: 0.91 }
        } else {
            ModerationOutcome::Safe { unsafe_score: 0.05 }
        }
    };
    let result = resolve_with_moderation(&workspace_files, &moderator)
        .expect("resolve must succeed even when content-safety flags an image");

    // The image must still be in `bundled_paths` — the resolver doesn't
    // strip rejected images, it surfaces a violation. Aggregate retry runs
    // the full pipeline again (INV-7.8.6); rejected images stay listed for
    // the renderer to label.
    assert!(
        result.bundled_paths.contains(&"images/sus.png".to_string()),
        "rejected image should still be in bundled_paths; got {:?}",
        result.bundled_paths
    );

    // The exact match: ContentSafety { path: "images/sus.png", confidence: 0.91 }.
    let found = result
        .violations
        .iter()
        .find(|v| matches!(v, Violation::ContentSafety { path, .. } if path == "images/sus.png"));
    let violation = found.expect_or_log_violations(&result.violations);
    if let Violation::ContentSafety { path, confidence } = violation {
        assert_eq!(path, "images/sus.png");
        assert!(
            (*confidence - 0.91).abs() < 1e-6,
            "confidence should round-trip the injected score; got {confidence}"
        );
    } else {
        unreachable!("matched ContentSafety above");
    }

    // No spurious violations — we didn't add any orphans or missing refs.
    let spurious: Vec<&Violation> = result
        .violations
        .iter()
        .filter(|v| !matches!(v, Violation::ContentSafety { .. }))
        .collect();
    assert!(
        spurious.is_empty(),
        "no missing/unused violations expected; got {spurious:?}"
    );
}

#[test]
fn content_safety_skipped_outcome_does_not_violate() {
    // The Skipped outcome models the model-not-loaded path that
    // `default_moderator` falls through to when `cargo test` runs without a
    // bundled ONNX model. Pin the "no violation" contract so adding the
    // content-safety pass doesn't accidentally fail integration tests on
    // CI runners without the asset.
    let mut workspace_files = BTreeMap::new();
    workspace_files.insert(
        "overlay.omni".into(),
        br#"<widget><template><div/></template><style>.x{background:url(images/clean.png)}</style></widget>"#.to_vec(),
    );
    workspace_files.insert(
        "images/clean.png".into(),
        vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
    );

    let moderator =
        |_path: &str, _bytes: &[u8]| -> ModerationOutcome { ModerationOutcome::Skipped };
    let result = resolve_with_moderation(&workspace_files, &moderator)
        .expect("resolve must succeed when moderator skips");

    assert!(
        !result
            .violations
            .iter()
            .any(|v| matches!(v, Violation::ContentSafety { .. })),
        "Skipped outcome must NOT push a ContentSafety violation; got {:?}",
        result.violations
    );
}

/// Tiny helper trait so the assertion above renders the full `violations`
/// vec on miss without inflating the test body. Mirrors the assertion
/// chrome the OWI-40 tests already use (`format!("{:?}", violations)` on
/// the `assert!` macro).
trait ExpectVecOption<'a, T> {
    fn expect_or_log_violations(self, all: &[Violation]) -> &'a T;
}

impl<'a> ExpectVecOption<'a, Violation> for Option<&'a Violation> {
    fn expect_or_log_violations(self, all: &[Violation]) -> &'a Violation {
        match self {
            Some(v) => v,
            None => panic!(
                "expected ContentSafety violation; full violations vec: {:?}",
                all
            ),
        }
    }
}

#[test]
fn transitive_theme_walk_with_cycle_guard() {
    let mut workspace_files = BTreeMap::new();
    workspace_files.insert(
        "overlay.omni".into(),
        br#"<widget><theme src="theme.css"/><template><div/></template></widget>"#.to_vec(),
    );
    // theme.css references both an image and itself — the self-reference is
    // a cycle the resolver must guard against (INV-7.8.3).
    workspace_files.insert(
        "theme.css".into(),
        b".x{background:url(images/a.png)} .y{background:url(theme.css)}".to_vec(),
    );
    workspace_files.insert("images/a.png".into(), vec![0]);

    let result = resolve(&workspace_files).expect("resolve must succeed without infinite loop");
    assert!(
        result.bundled_paths.contains(&"images/a.png".to_string()),
        "transitive walk must discover images/a.png via theme.css; got {:?}",
        result.bundled_paths
    );
    // theme.css itself is bundled (it's a referenced theme file).
    assert!(
        result.bundled_paths.contains(&"theme.css".to_string()),
        "theme.css must be bundled as a referenced theme; got {:?}",
        result.bundled_paths
    );
}
