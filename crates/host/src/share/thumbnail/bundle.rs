//! Thumbnail path for signed `.omnipkg` bundles.
//!
//! Call order is fixed by invariants #6a + #19b (manifest-only fast path →
//! version + kind pre-flight → signed unpack → streaming extraction → render).
//! Ordering is load-bearing: malicious bundles must be rejected before any
//! filesystem cost.
//!
//! ## Observed-API notes (recorded for Task 6 integration work)
//!
//! - `identity::unpack_signed_bundle(bytes, Option<&PublicKey>, &BundleLimits)`
//!   takes `BundleLimits` (not a separate `IdentityLimits` — that type doesn't
//!   exist in the shipped crate). Passing `None` for `expected_pubkey` at the
//!   thumbnail boundary because thumbnail generation runs pre-TOFU (spec §4).
//! - `bundle::Manifest.resource_kinds` is
//!   `Option<BTreeMap<String, ResourceKind>>` — the "kind" is the map key
//!   (a `String`) and the sanitizer's handler registry (`theme` / `font` /
//!   `image` / `overlay`) is the closed vocabulary. `ResourceKind` itself is
//!   a struct (`dir`, `extensions`, `max_size_bytes`), not an enum.
//! - `SignedBundle::files()` iterates an in-memory `BTreeMap`; the true
//!   streaming iterator lives one layer lower in `bundle::unpack`. We
//!   still write files one-at-a-time to the `TempDir` so peak filesystem
//!   pressure is bounded by the largest single file.
//! - `unpack_manifest` internally rejects `schema_version != 1` today, but we
//!   keep the explicit pre-flight here: if a future `bundle` release
//!   accepts v2 structurally, this entry point still fails closed until the
//!   renderer is audited for the new schema (invariant #6b).

use bundle::{unpack_manifest, BundleLimits, Manifest};
use identity::unpack_signed_bundle;
use tempfile::TempDir;

use super::{render_omni_to_png, ThumbnailConfig, ThumbnailError};
use crate::omni::parser::parse_omni_with_diagnostics;

/// Render a PNG thumbnail for a signed `.omnipkg` bundle.
///
/// Accepts the raw signed-bundle bytes (the payload of
/// `pack_signed_bundle`). Verification, extraction, and render happen
/// in-memory plus a single [`TempDir`] that is dropped on return — the
/// caller's workspace is never touched.
pub fn generate_for_bundle(
    signed_bundle_bytes: &[u8],
    config: &ThumbnailConfig,
) -> Result<Vec<u8>, ThumbnailError> {
    let limits = BundleLimits::DEFAULT;

    // 1. Manifest-only fast path (invariant #19b) — zero file I/O.
    let manifest: Manifest =
        unpack_manifest(signed_bundle_bytes, &limits).map_err(ThumbnailError::Bundle)?;

    // 2. schema_version pre-flight (invariant #6b).
    if !is_supported_schema_version(manifest.schema_version) {
        return Err(ThumbnailError::UnsupportedSchemaVersion {
            version: manifest.schema_version,
        });
    }

    // 3. resource_kinds pre-flight (invariant #19 closed vocabulary). Only the
    //    declared kind names are gated here — handler dispatch happens inside
    //    `omni-sanitize` for the install path; at thumbnail time we fail fast
    //    on any kind the renderer/sanitizer wouldn't recognize.
    if let Some(kinds) = manifest.resource_kinds.as_ref() {
        for kind_name in kinds.keys() {
            if !is_supported_resource_kind(kind_name) {
                return Err(ThumbnailError::UnsupportedKind {
                    kind: kind_name.clone(),
                });
            }
        }
    }

    // 4. Signed unpack (invariant #9 trust-anchor construction). Holding the
    //    returned `SignedBundle` IS the JWS-verified + hash-matched proof.
    //    `expected_pubkey = None`: thumbnail generation runs before TOFU
    //    anchoring; the signature is verified against the key embedded in the
    //    JWS header regardless.
    let signed = unpack_signed_bundle(signed_bundle_bytes, None, &limits)
        .map_err(ThumbnailError::Identity)?;

    // 5. Streaming extraction into a TempDir. Files land under
    //    `<tempdir>/overlays/<overlay_name>/<relative_path>` so
    //    `resolve_theme_path` will find any declared theme.
    let overlay_name = bundle_overlay_name(signed.manifest());
    let tmp = TempDir::new().map_err(ThumbnailError::Io)?;
    let overlay_root = crate::workspace::structure::overlay_dir(tmp.path(), &overlay_name);

    for (rel_path, bytes) in signed.files() {
        let dest = overlay_root.join(rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(ThumbnailError::Io)?;
        }
        std::fs::write(&dest, bytes).map_err(ThumbnailError::Io)?;
    }

    // 6. Parse the entry `.omni` and render.
    let entry_path = overlay_root.join(&signed.manifest().entry_overlay);
    let entry_source = std::fs::read_to_string(&entry_path).map_err(ThumbnailError::Io)?;
    // `parse_omni_with_diagnostics` returns `(Option<OmniFile>, Vec<ParseError>)`;
    // we treat any Error-severity diagnostic as fatal for thumbnail generation.
    let (parsed, diagnostics) = parse_omni_with_diagnostics(&entry_source);
    let errors: Vec<_> = diagnostics
        .into_iter()
        .filter(|d| matches!(d.severity, crate::omni::parser::Severity::Error))
        .collect();
    if !errors.is_empty() {
        return Err(ThumbnailError::RenderFailed {
            detail: format!(
                "parse entry_overlay {:?}: {errors:?}",
                signed.manifest().entry_overlay
            ),
        });
    }
    let omni_file = parsed.ok_or_else(|| ThumbnailError::RenderFailed {
        detail: format!(
            "parse entry_overlay {:?} produced no OmniFile",
            signed.manifest().entry_overlay
        ),
    })?;

    render_omni_to_png(&omni_file, tmp.path(), &overlay_name, config)
    // `tmp` drops here; the TempDir is deleted on scope exit.
}

/// Render a PNG thumbnail for an UNPACKED overlay sitting in the user's
/// workspace at `data_dir/overlays/<overlay_name>/`.
///
/// Added for the upload-flow-redesign save-time `.omni-preview.png` hook
/// (spec §8.3 / Wave A0 Task A0.2-3-4). Unlike [`generate_for_bundle`] this
/// path skips the signed-bundle unpack/verify dance — the workspace is the
/// trust boundary at this layer (the file was just written by the host's own
/// `file.write` handler), so no extra crypto is needed.
///
/// `overlay_dir` is the full path to the overlay folder, e.g.
/// `%APPDATA%\Omni\overlays\marathon-hud`. The function derives `data_dir`
/// (two parents up: `…\Omni`) and `overlay_name` (the leaf component) and
/// delegates to the same [`render_omni_to_png`] core path used by
/// [`generate_for_bundle`].
///
/// Reads `overlay_dir/overlay.omni` from disk on every call (no caching) —
/// the save-time hook fires on the file just written, so the disk read sees
/// the new contents.
pub fn generate_for_workspace_overlay(
    overlay_dir: &std::path::Path,
) -> Result<Vec<u8>, ThumbnailError> {
    use crate::omni::parser::Severity;

    // Derive `data_dir` and `overlay_name`. `overlay_dir` is expected to be of
    // the form `<data_dir>/overlays/<name>`; defensive fallbacks let an
    // unusual layout (e.g. tests with `<tempdir>/<name>` only) still fail
    // with a structured error rather than panicking.
    let overlay_name = overlay_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| ThumbnailError::RenderFailed {
            detail: format!(
                "overlay_dir has no UTF-8 leaf component: {}",
                overlay_dir.display()
            ),
        })?;
    let data_dir = overlay_dir
        .parent() // `<data_dir>/overlays`
        .and_then(|p| p.parent()) // `<data_dir>`
        .ok_or_else(|| ThumbnailError::RenderFailed {
            detail: format!(
                "overlay_dir is not under a data_dir/overlays/ layout: {}",
                overlay_dir.display()
            ),
        })?;

    let entry_path = overlay_dir.join("overlay.omni");
    let entry_source = std::fs::read_to_string(&entry_path).map_err(ThumbnailError::Io)?;

    let (parsed, diagnostics) = parse_omni_with_diagnostics(&entry_source);
    let errors: Vec<_> = diagnostics
        .into_iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    if !errors.is_empty() {
        return Err(ThumbnailError::RenderFailed {
            detail: format!("parse {}: {errors:?}", entry_path.display()),
        });
    }
    let omni_file = parsed.ok_or_else(|| ThumbnailError::RenderFailed {
        detail: format!("parse {} produced no OmniFile", entry_path.display()),
    })?;

    let config = ThumbnailConfig::default();
    render_omni_to_png(&omni_file, data_dir, overlay_name, &config)
}

/// Derive a stable `overlay_name` for the workspace layout. Uses the manifest
/// `name` lowercased and stripped of path-invalid characters; falls back to
/// `"bundle"` if nothing usable remains. Bundles only ever live under
/// `<tempdir>/overlays/<overlay_name>/…` for the duration of this call, so
/// the choice only needs to be a valid single path segment.
fn bundle_overlay_name(manifest: &Manifest) -> String {
    slugify(&manifest.name)
}

/// Slug helper: ASCII-lowercase alnum/`-`/`_`, trim dashes, cap at 64 bytes
/// (defensive vs. Windows `MAX_PATH = 260`), fall back to `"bundle"` on empty.
/// Post-map the slug is ASCII, so a byte-bounded truncate is UTF-8 safe.
fn slugify(name: &str) -> String {
    const MAX_SLUG_BYTES: usize = 64;
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        return "bundle".to_string();
    }
    let capped = if trimmed.len() > MAX_SLUG_BYTES {
        &trimmed[..MAX_SLUG_BYTES]
    } else {
        trimmed
    };
    if capped.is_empty() {
        "bundle".to_string()
    } else {
        capped.to_string()
    }
}

/// Extend per umbrella invariant #6b — one version axis per format.
fn is_supported_schema_version(version: u32) -> bool {
    version == 1
}

/// Closed vocabulary pulled from `omni-sanitize`'s handler registry (theme /
/// font / image / overlay). Kept as a literal-match list here so adding a new
/// handler upstream without auditing the thumbnail path is impossible without
/// updating this function — intentional (invariant #19).
fn is_supported_resource_kind(kind: &str) -> bool {
    matches!(kind, "theme" | "font" | "image" | "overlay")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_supported_schema_version_accepts_v1() {
        assert!(is_supported_schema_version(1));
    }

    #[test]
    fn is_supported_schema_version_rejects_others() {
        assert!(!is_supported_schema_version(0));
        assert!(!is_supported_schema_version(2));
        assert!(!is_supported_schema_version(u32::MAX));
    }

    #[test]
    fn is_supported_resource_kind_covers_shipped_handlers() {
        // These four match `sanitize::handlers::HANDLERS` at the time of
        // writing. If omni-sanitize grows a new handler, this literal match
        // is the gate — the thumbnail path will reject the new kind until
        // `is_supported_resource_kind` is updated as part of the same audit.
        assert!(is_supported_resource_kind("theme"));
        assert!(is_supported_resource_kind("font"));
        assert!(is_supported_resource_kind("image"));
        assert!(is_supported_resource_kind("overlay"));
    }

    #[test]
    fn is_supported_resource_kind_rejects_unknown() {
        assert!(!is_supported_resource_kind("sounds"));
        assert!(!is_supported_resource_kind(""));
        assert!(!is_supported_resource_kind("Theme")); // case-sensitive
    }

    #[test]
    fn bundle_overlay_name_slugifies_manifest_name() {
        let mut m = sample_manifest();
        m.name = "My Cool Theme!".into();
        // Spaces and '!' map to '-', trailing '-' is trimmed.
        assert_eq!(bundle_overlay_name(&m), "my-cool-theme");
    }

    #[test]
    fn bundle_overlay_name_preserves_valid_chars() {
        let mut m = sample_manifest();
        m.name = "dark_mode-v2".into();
        assert_eq!(bundle_overlay_name(&m), "dark_mode-v2");
    }

    #[test]
    fn slugify_caps_length_at_64_bytes() {
        let long = "a".repeat(200);
        let s = slugify(&long);
        assert!(s.len() <= 64, "slug should be capped: {}", s.len());
        assert!(!s.is_empty());
    }

    #[test]
    fn bundle_overlay_name_caps_length() {
        let mut m = sample_manifest();
        m.name = "a".repeat(200);
        let name = bundle_overlay_name(&m);
        assert!(name.len() <= 64);
        assert!(!name.is_empty());
    }

    #[test]
    fn bundle_overlay_name_falls_back_when_empty() {
        let mut m = sample_manifest();
        m.name = "!!!".into();
        assert_eq!(bundle_overlay_name(&m), "bundle");
    }

    fn sample_manifest() -> Manifest {
        Manifest {
            schema_version: 1,
            name: "Sample".into(),
            version: "1.0.0".parse().unwrap(),
            omni_min_version: "0.1.0".parse().unwrap(),
            description: String::new(),
            tags: Vec::new(),
            license: "MIT".into(),
            entry_overlay: "overlay.omni".into(),
            default_theme: None,
            sensor_requirements: Vec::new(),
            files: vec![bundle::FileEntry {
                path: "overlay.omni".into(),
                sha256: [0u8; 32],
            }],
            resource_kinds: None,
        }
    }
}
