//! Integration tests for `share::dep_resolver` (OWI-40 / Task A1.6).
//!
//! Spec: docs/superpowers/specs/2026-04-21-upload-flow-redesign-design.md §8.4
//! steps 1-6, INV-7.8.1, INV-7.8.2, INV-7.8.3, INV-7.8.4 (missing-refs +
//! unused-files only — content-safety lands in Wave B1.5).
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

use omni_host::share::dep_resolver::{resolve, Violation};
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
        result
            .bundled_paths
            .contains(&"images/a.png".to_string()),
        "transitive walk must discover images/a.png via theme.css; got {:?}",
        result.bundled_paths
    );
    // theme.css itself is bundled (it's a referenced theme file).
    assert!(
        result
            .bundled_paths
            .contains(&"theme.css".to_string()),
        "theme.css must be bundled as a referenced theme; got {:?}",
        result.bundled_paths
    );
}
