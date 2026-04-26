//! Dependency resolver for overlay/theme upload bundling (OWI-40 / Task A1.6).
//!
//! Spec: docs/superpowers/specs/2026-04-21-upload-flow-redesign-design.md §8.4
//! steps 1-6, INV-7.8.1, INV-7.8.2, INV-7.8.3, INV-7.8.4 (missing-refs +
//! unused-files only — content-safety lands in Wave B1.5 / OWI-54).
//!
//! ## What this resolves
//!
//! Given a `BTreeMap<workspace-relative path, bytes>` (the same shape
//! `share::upload::walk_bundle` produces), `resolve` walks the overlay's
//! references transitively and returns:
//!
//! - `bundled_paths`: every file the bundle should ship (overlay + referenced
//!   themes + referenced images + referenced fonts). Order: `overlay.omni`
//!   first, then refs in deterministic discovery order.
//! - `violations`: `MissingRef` for refs whose target isn't in
//!   `workspace_files`, `UnusedFile` for files under `images/` / `fonts/` or
//!   referenced theme.css paths that nothing references.
//!
//! ## Reference categories (INV-7.8.1)
//!
//! Resolved starting from `overlay.omni`:
//!
//! 1. **Theme refs** — `<theme src="..."/>` elements anywhere in the overlay
//!    XML. Discovered transitively: a theme's own CSS body can reference
//!    images via `url(...)`, but inter-theme `@import` is rejected by
//!    `omni-sanitize::handlers::theme` and therefore does not need
//!    transitive theme resolution here (spec §7.8.1 explicit).
//! 2. **Font refs** — `<font src="..."/>` elements anywhere in the overlay
//!    XML.
//! 3. **Image refs** — CSS `url(...)` values found in inline `<style>` blocks
//!    inside the overlay AND inside every referenced theme.css body.
//!
//! ## Cycle guard (INV-7.8.3)
//!
//! `visited: HashSet<String>` over the workspace path of every theme.css
//! we've already walked. A theme that references itself or a parent theme
//! is silently skipped on the second visit instead of recursing forever.
//!
//! ## Permissive XML scan
//!
//! The strict overlay structural validator lives in
//! `omni-sanitize::handlers::overlay`. Here we only need to *find* refs, so
//! we walk every `quick_xml` event regardless of nesting depth and pick out
//! `<theme src>`, `<font src>`, and `<style>` blocks. This means refs nested
//! inside a `<widget>` (as the resolver tests use) work alongside the
//! top-level forms accepted by the production parser. The downstream
//! sanitizer is the strict gate — this resolver only enumerates what the
//! bundle needs to carry.
//!
//! ## Path semantics
//!
//! `<theme src>`, `<font src>`, and CSS `url(...)` values are interpreted
//! verbatim as workspace-relative paths (forward slashes, no leading `/`,
//! no `..` segments). The strict checks live in
//! `omni-sanitize::handlers::overlay::validate_theme_src` and
//! `omni-sanitize::handlers::theme::validate_url`. Here we only check
//! existence in `workspace_files`; if the strict sanitizer would reject the
//! path, the upload pipeline catches it later regardless.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use lightningcss::stylesheet::{ParserOptions, StyleSheet};
use quick_xml::events::Event;
use quick_xml::Reader;

const OVERLAY_PATH: &str = "overlay.omni";

/// A single dependency-resolution failure. Variants match the renderer's
/// `ViolationKind` enum (see
/// `apps/desktop/renderer/components/omni/upload-dialog/steps/packing-violations-card.tsx`).
///
/// `MissingRef` and `UnusedFile` ship in this Wave A1 task. `ContentSafety`
/// arrives in Wave B1.5 (OWI-54) when `omni-host` integrates the ONNX
/// moderation crate; the renderer already accepts the third category so
/// the wire shape doesn't churn later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Violation {
    /// Overlay (or a referenced theme.css) references a workspace path that
    /// doesn't exist in the workspace file map. INV-7.8.4 missing-refs.
    MissingRef { path: String },
    /// File present under `images/`, `fonts/`, or as a referenced theme.css
    /// that no overlay/theme references. Orphans are a covert-distribution
    /// vector (spec §7.8.4); rejecting them keeps every shipped byte
    /// reachable from the overlay. INV-7.8.4 unused-files.
    UnusedFile { path: String },
    // ContentSafety { path, confidence } — added in Wave B1.5 (OWI-54).
}

/// Outcome of `resolve`. `bundled_paths` is the deterministic file list the
/// caller should ship; `violations` is the aggregate list (INV-7.3.7 — no
/// fail-fast inside the Dependency Check stage). Both lists are empty on a
/// fully-clean workspace.
#[derive(Debug, Clone)]
pub struct ResolveResult {
    pub bundled_paths: Vec<String>,
    pub violations: Vec<Violation>,
}

/// Resolver-internal failure for malformed inputs. Today only XML parse
/// errors on `overlay.omni` surface as `Err` — the strict validator in
/// `omni-sanitize::handlers::overlay` re-runs after this resolver so any
/// XML that parses here will be re-checked there. Missing files are NOT
/// errors; they're `Violation::MissingRef` entries inside `Ok(...)`.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("workspace is missing the entry overlay file ({entry:?})")]
    MissingEntryOverlay { entry: String },

    #[error("invalid CSS in {path}: {detail}")]
    InvalidCss { path: String, detail: String },

    #[error("invalid overlay XML: {0}")]
    InvalidOverlayXml(String),
}

/// Resolve overlay + theme + image + font references against the workspace.
///
/// `workspace_files` is the same map shape `share::upload::walk_bundle`
/// produces — keys are workspace-relative paths with forward slashes, values
/// are file bytes. The resolver does not touch the filesystem.
///
/// The function returns `Err(ResolveError::MissingEntryOverlay)` when
/// `workspace_files` does not contain `overlay.omni`. Theme-only artifacts
/// (no `overlay.omni`) skip the resolver entirely — this entry point is
/// only invoked for `ArtifactKind::Bundle`.
pub fn resolve(
    workspace_files: &BTreeMap<String, Vec<u8>>,
) -> Result<ResolveResult, ResolveError> {
    let overlay_bytes = workspace_files
        .get(OVERLAY_PATH)
        .ok_or_else(|| ResolveError::MissingEntryOverlay {
            entry: OVERLAY_PATH.to_string(),
        })?;

    // Step 1+2: parse overlay.omni and extract <theme src>, <font src>, and
    // any inline <style> bodies. The XML scan is permissive — it walks every
    // event regardless of nesting because the resolver tests legitimately
    // place <theme src> inside <widget> while the strict format places it at
    // the top level. The strict structural gate runs later in
    // `omni-sanitize::handlers::overlay`.
    let overlay_refs = extract_overlay_refs(overlay_bytes)
        .map_err(ResolveError::InvalidOverlayXml)?;

    // Resolved-refs ledger: deterministic insertion order via Vec, dedup via
    // a parallel BTreeSet. The bundle output starts with overlay.omni and
    // appends each unique ref in the order it was first discovered.
    let mut bundled: Vec<String> = vec![OVERLAY_PATH.to_string()];
    let mut bundled_set: BTreeSet<String> = BTreeSet::new();
    bundled_set.insert(OVERLAY_PATH.to_string());

    let mut violations: Vec<Violation> = Vec::new();
    let mut visited_themes: HashSet<String> = HashSet::new();

    // Step 3: inline <style> URLs become image refs. The CSS is parsed via
    // lightningcss to validate well-formedness; the actual url() values are
    // extracted by the same substring scan `omni-sanitize::handlers::theme`
    // uses (the lightningcss `visitor` feature is gated and not enabled in
    // this workspace, and the substring scan is the production-validated
    // pattern).
    for style_body in overlay_refs.inline_styles.iter() {
        // Validate parse-ability so a bundle never ships unparseable CSS the
        // sanitizer would later reject. Parse failures here surface as
        // `InvalidCss { path: "overlay.omni" }` so the renderer's error card
        // can point the user at the right file.
        let css_str = std::str::from_utf8(style_body)
            .map_err(|e| ResolveError::InvalidCss {
                path: OVERLAY_PATH.to_string(),
                detail: format!("utf8: {e}"),
            })?;
        StyleSheet::parse(css_str, ParserOptions::default()).map_err(|e| {
            ResolveError::InvalidCss {
                path: OVERLAY_PATH.to_string(),
                detail: format!("parse: {e}"),
            }
        })?;
        for url in scan_css_urls(css_str) {
            register_ref(
                &url,
                workspace_files,
                &mut bundled,
                &mut bundled_set,
                &mut violations,
            );
        }
    }

    // Step 2 (cont.): font refs — register existence, no transitive walk.
    for font_path in overlay_refs.font_srcs.iter() {
        register_ref(
            font_path,
            workspace_files,
            &mut bundled,
            &mut bundled_set,
            &mut violations,
        );
    }

    // Step 4: walk every <theme src> recursively. A referenced theme.css's
    // body can carry url(...) image refs, which transitively bundle (INV-7.8.2).
    // `visited_themes` guards against cycles (INV-7.8.3).
    for theme_path in overlay_refs.theme_srcs.iter() {
        walk_theme(
            theme_path,
            workspace_files,
            &mut bundled,
            &mut bundled_set,
            &mut violations,
            &mut visited_themes,
        );
    }

    // Step 6: any file under images/ or fonts/ — or any *.css living at a
    // workspace path that wasn't followed via <theme src> — that the resolver
    // never registered is an orphan. INV-7.8.4 unused-files: orphans are a
    // covert-distribution vector (spec §7.8.4) and reject the bundle.
    for path in workspace_files.keys() {
        if bundled_set.contains(path) {
            continue;
        }
        if is_resource_path(path) {
            violations.push(Violation::UnusedFile { path: path.clone() });
        }
    }

    Ok(ResolveResult {
        bundled_paths: bundled,
        violations,
    })
}

/// Refs collected from a single overlay XML pass.
struct OverlayRefs {
    theme_srcs: Vec<String>,
    font_srcs: Vec<String>,
    inline_styles: Vec<Vec<u8>>,
}

/// Permissive overlay scan — finds `<theme src>`, `<font src>`, and `<style>`
/// bodies anywhere in the document. Records appear in document order so the
/// resolved-refs Vec is deterministic.
fn extract_overlay_refs(bytes: &[u8]) -> Result<OverlayRefs, String> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = false;

    let mut theme_srcs = Vec::new();
    let mut font_srcs = Vec::new();
    let mut inline_styles: Vec<Vec<u8>> = Vec::new();

    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(format!("xml parse: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    b"theme" => {
                        if let Some(src) = read_src_attr(e) {
                            theme_srcs.push(src);
                        }
                    }
                    b"font" => {
                        if let Some(src) = read_src_attr(e) {
                            font_srcs.push(src);
                        }
                    }
                    b"style" => {
                        // Body lives between this start tag's `>` and the
                        // matching `</style>` `<`. quick_xml's
                        // `buffer_position` after `read_event_into` for a
                        // Start event sits just after the `>`, so capture it
                        // as the body start, then scan forward to the
                        // matching End event.
                        let body_start = reader.buffer_position();
                        let body_end = consume_until_close(&mut reader, b"style")?;
                        if body_end >= body_start {
                            let raw = &bytes[body_start as usize..body_end as usize];
                            inline_styles.push(raw.to_vec());
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let name = e.name();
                match name.as_ref() {
                    b"theme" => {
                        if let Some(src) = read_src_attr(e) {
                            theme_srcs.push(src);
                        }
                    }
                    b"font" => {
                        if let Some(src) = read_src_attr(e) {
                            font_srcs.push(src);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(OverlayRefs {
        theme_srcs,
        font_srcs,
        inline_styles,
    })
}

/// Read the `src` attribute off a quick_xml Start/Empty event. Returns the
/// attribute value as a UTF-8 String (workspace-relative path); returns
/// `None` if the attribute is absent or the value isn't valid UTF-8.
fn read_src_attr(e: &quick_xml::events::BytesStart) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == b"src")
        .and_then(|a| String::from_utf8(a.value.into_owned()).ok())
        .filter(|s| !s.is_empty())
}

/// Consume events from `reader` until the matching close tag for `tag`,
/// returning the byte offset of the `<` of the close tag (i.e. the body end
/// position). Mirrors `omni-sanitize::handlers::overlay::skip_to_close` but
/// works against `Reader<&[u8]>` and is simpler since we don't track depth
/// on nested same-name tags (CSS doesn't nest `<style>` inside `<style>`).
fn consume_until_close(
    reader: &mut Reader<&[u8]>,
    tag: &[u8],
) -> Result<u64, String> {
    let mut buf = Vec::new();
    loop {
        let before = reader.buffer_position();
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(format!("xml parse inside <{}>: {e}", String::from_utf8_lossy(tag))),
            Ok(Event::Eof) => {
                return Err(format!(
                    "unterminated <{}> body",
                    String::from_utf8_lossy(tag)
                ));
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == tag => {
                buf.clear();
                return Ok(before);
            }
            _ => {}
        }
        buf.clear();
    }
}

/// Walk a referenced theme.css. Adds the theme to the bundle, parses its
/// body via lightningcss, and registers every `url(...)` value as a
/// transitive image ref. Cycle-guarded via `visited_themes` (INV-7.8.3).
fn walk_theme(
    path: &str,
    workspace_files: &BTreeMap<String, Vec<u8>>,
    bundled: &mut Vec<String>,
    bundled_set: &mut BTreeSet<String>,
    violations: &mut Vec<Violation>,
    visited_themes: &mut HashSet<String>,
) {
    if !visited_themes.insert(path.to_string()) {
        // Already walked — cycle guard.
        return;
    }
    // Register the theme itself so it's bundled.
    let exists = workspace_files.contains_key(path);
    if !exists {
        violations.push(Violation::MissingRef {
            path: path.to_string(),
        });
        return;
    }
    if bundled_set.insert(path.to_string()) {
        bundled.push(path.to_string());
    }

    let bytes = &workspace_files[path];
    let css_str = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => {
            // Non-UTF-8 CSS will be caught by the sanitizer; here we just
            // skip url() extraction for it.
            return;
        }
    };
    // Parse-validate so we surface broken CSS as a violation rather than
    // silently bundling it. Parse failures aren't part of the missing/unused
    // contract today — the sanitizer rejects them downstream — so here we
    // just skip url() extraction on parse error.
    if StyleSheet::parse(css_str, ParserOptions::default()).is_err() {
        return;
    }
    for url in scan_css_urls(css_str) {
        // CSS that references another *.css path becomes a recursive theme
        // walk (covers the test case of `theme.css` referencing itself).
        // Anything else is a leaf resource ref.
        if url.ends_with(".css") {
            walk_theme(
                &url,
                workspace_files,
                bundled,
                bundled_set,
                violations,
                visited_themes,
            );
        } else {
            register_ref(&url, workspace_files, bundled, bundled_set, violations);
        }
    }
}

/// Register a leaf resource ref (image or font). Records `MissingRef` if the
/// target isn't in `workspace_files`; otherwise dedups + appends to the
/// bundle list.
fn register_ref(
    path: &str,
    workspace_files: &BTreeMap<String, Vec<u8>>,
    bundled: &mut Vec<String>,
    bundled_set: &mut BTreeSet<String>,
    violations: &mut Vec<Violation>,
) {
    if workspace_files.contains_key(path) {
        if bundled_set.insert(path.to_string()) {
            bundled.push(path.to_string());
        }
    } else {
        violations.push(Violation::MissingRef {
            path: path.to_string(),
        });
    }
}

/// Whether a workspace path counts as a "bundleable resource" for the
/// unused-file check. Anything under `images/` or `fonts/` is in scope; CSS
/// files (potential transitive themes) are also in scope. The overlay
/// itself is exempt because we always include it. Other paths (e.g. a
/// stray top-level `README.md`) are out of scope — they'd be rejected by
/// the sanitizer's per-handler dispatch later, not by this resolver.
fn is_resource_path(path: &str) -> bool {
    if path == OVERLAY_PATH {
        return false;
    }
    path.starts_with("images/")
        || path.starts_with("fonts/")
        || path.ends_with(".css")
}

/// Substring scan for `url(...)` values inside a CSS body. Mirrors the
/// production scanner in `omni-sanitize::handlers::theme::scan_urls`. Returns
/// the trimmed, unquoted URL value of every `url(...)` form found. Malformed
/// `url(` (no closing `)`) terminates the scan early; the strict sanitizer
/// rejects the CSS later regardless.
fn scan_css_urls(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let lower = src.to_ascii_lowercase();
    let mut i = 0;
    while let Some(idx) = lower[i..].find("url(") {
        let start = i + idx + 4;
        let rest = &src[start..];
        let Some(end) = rest.find(')') else {
            break;
        };
        let arg = rest[..end]
            .trim()
            .trim_matches(|c| c == '\'' || c == '"');
        if !arg.is_empty() {
            out.push(arg.to_string());
        }
        i = start + end + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_css_urls_handles_quotes_and_spaces() {
        let css = "a{background:url(\"images/a.png\")} b{background:url( 'images/b.png' )}";
        let urls = scan_css_urls(css);
        assert_eq!(urls, vec!["images/a.png".to_string(), "images/b.png".to_string()]);
    }

    #[test]
    fn is_resource_path_classification() {
        assert!(is_resource_path("images/a.png"));
        assert!(is_resource_path("fonts/x.ttf"));
        assert!(is_resource_path("themes/dark.css"));
        assert!(is_resource_path("nested/dir.css"));
        assert!(!is_resource_path("overlay.omni"));
        assert!(!is_resource_path("README.md"));
    }

    #[test]
    fn empty_workspace_missing_overlay_errors() {
        let map: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        let err = resolve(&map).expect_err("missing overlay must error");
        assert!(matches!(err, ResolveError::MissingEntryOverlay { .. }));
    }
}
