//! Compile-time embedded `.omni` assets shipped with the host.
//!
//! These assets mirror the shape of [`super::default::DEFAULT_OMNI`] — a single
//! `<widget>` element containing `<template>` and `<style>` blocks parsed by
//! [`super::parser::parse_omni`] and rendered through
//! [`super::html_builder::build_initial_html`]. They are **not** separate
//! HTML/CSS/manifest file trees; there is only one render path in the host
//! (architectural invariant #8), and every piece of authored content — default,
//! reference, user-installed — flows through it.
//!
//! # Reference overlay
//!
//! [`REFERENCE_OVERLAY_OMNI`] is a representative HUD used by
//! `crate::share::thumbnail` when rendering theme thumbnails: themes are
//! CSS-only, so the host renders them against a fixed overlay to produce a
//! visually-comparable PNG. The overlay exercises every CSS variable declared
//! by [`super::default::DEFAULT_THEME_CSS`] so arbitrary themes remain
//! distinguishable at thumbnail size.

/// Reference overlay embedded at compile time.
///
/// Loaded from `reference_overlay.omni` via [`include_str!`]. Parsed with
/// [`super::parser::parse_omni`] at thumbnail-generation time; never extracted
/// to a temp directory as a standalone HTML tree.
pub const REFERENCE_OVERLAY_OMNI: &str = include_str!("reference_overlay.omni");

#[cfg(test)]
mod tests {
    use super::super::parser;
    use super::*;

    /// The 9 CSS variable names declared by `DEFAULT_THEME_CSS` in `default.rs`.
    /// The reference overlay must reference each at least once so theme
    /// swaps produce visually distinguishable thumbnails.
    const DEFAULT_THEME_VARS: &[&str] = &[
        "--bg",
        "--bg-light",
        "--text",
        "--text-dim",
        "--accent",
        "--warning",
        "--critical",
        "--font",
        "--font-size",
    ];

    #[test]
    fn reference_overlay_parses() {
        let file = parser::parse_omni(REFERENCE_OVERLAY_OMNI)
            .expect("reference overlay must parse without errors");
        assert_eq!(file.widgets.len(), 1);
        assert_eq!(file.widgets[0].id, "reference-hud");
        assert!(file.widgets[0].enabled);
        assert_eq!(file.theme_src.as_deref(), Some("theme.css"));
    }

    #[test]
    fn reference_overlay_exercises_all_default_theme_vars() {
        for var in DEFAULT_THEME_VARS {
            let needle = format!("var({var})");
            assert!(
                REFERENCE_OVERLAY_OMNI.contains(&needle),
                "reference overlay does not consume CSS variable `{var}` via \
                 `var({var})`; theme swaps would be invisible on this axis at \
                 thumbnail size"
            );
        }
    }
}
