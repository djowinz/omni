//! Post-publish persistence — writes the per-artifact `.omni-publish.json`
//! sidecar (§8.1) AND upserts the workspace-global publish-index (§8.2)
//! after a successful publish/update.
//!
//! This is the writer half of the contract that `share::sidecar` (read) +
//! `share::publish_index` (read) + `useUploadMachine.detectMode` (consumer)
//! depend on to recognise a re-upload as an UPDATE rather than a fresh
//! artifact. Without this writer call, every upload would be treated as
//! `create` mode by the next dialog open — the gap that survived
//! upload-flow-redesign Wave A because OWI-31 only built the read helpers
//! and OWI-46 mocked the wire boundary instead of exercising this path.
//!
//! ## Design notes
//!
//! * **Pure function over a method-on-context** — the caller in
//!   `ws_messages::handle_publish` reads `ctx.data_dir`, `ctx.identity`,
//!   and parsed manifest fields and hands them in. This keeps the helper
//!   testable without constructing a full `ShareContext`.
//! * **Opportunistic, non-fatal failures** — sidecar and index write
//!   errors are logged at `WARN` and dropped. The publish itself succeeded
//!   server-side; refusing to surface that to the user because we couldn't
//!   touch local state would be worse than the missing prefill on the next
//!   open. INV-7.6.1's silent-restore path already handles a missing local
//!   sidecar (the index is consulted as a fallback).
//! * **Independent error paths** — sidecar and index writes don't share a
//!   transaction; one failing doesn't skip the other. They serve different
//!   recovery scenarios (sidecar = per-artifact state for that overlay's
//!   directory; index = workspace-global silent-restore source after
//!   sidecar deletion).

use std::path::Path;

use crate::share::publish_index::{self, PublishIndexEntry};
use crate::share::sidecar::{self, PublishSidecar};
use crate::share::upload::ArtifactKind;

/// Format a UNIX timestamp (seconds since epoch) as RFC 3339
/// `YYYY-MM-DDTHH:MM:SSZ`. Re-exposed from `ws_messages` to keep this
/// helper free of cross-module coupling. The hand-rolled formatter is the
/// one already in use by `mtime_to_iso` in `ws_messages.rs`; centralising
/// it here would require a wider refactor and isn't in scope for the
/// upload-flow follow-up.
fn format_unix_secs_as_iso8601_utc(secs: i64) -> String {
    if secs < 0 {
        return String::new();
    }
    let secs_u = secs as u64;
    let day = (secs_u / 86_400) as i64;
    let time_of_day = secs_u % 86_400;
    let hour = (time_of_day / 3_600) as u32;
    let minute = ((time_of_day % 3_600) / 60) as u32;
    let second = (time_of_day % 60) as u32;
    let z = day + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, m, d, hour, minute, second
    )
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_unix_secs_as_iso8601_utc(secs)
}

/// Inputs for `persist_publish_state`. Lifted into a struct so the call
/// site doesn't need a 9-arg function call.
pub struct PersistInputs<'a> {
    pub data_dir: &'a Path,
    pub kind: ArtifactKind,
    /// Workspace-relative path of the published artifact, e.g.
    /// `"overlays/marathon-hud"` (overlay) or `"themes/synth.css"`
    /// (theme). Matches the `workspace_path` field on
    /// `PublishablesEntry`.
    pub workspace_path: &'a str,
    pub pubkey_hex: &'a str,
    pub artifact_id: &'a str,
    pub version: &'a str,
    pub description: &'a str,
    pub tags: &'a [String],
    pub license: &'a str,
}

/// Write the per-artifact sidecar AND upsert the workspace-global publish
/// index. Both are idempotent — repeated calls with the same artifact
/// land in the same files and overwrite the previous row.
///
/// Errors are logged at `WARN` and discarded. The publish itself has
/// already succeeded; failing the WS handler because of a local-state
/// hiccup would be worse than the missing prefill on the next open.
pub fn persist_publish_state(inputs: &PersistInputs<'_>) {
    let now = now_iso();

    let sidecar_payload = PublishSidecar {
        artifact_id: inputs.artifact_id.to_string(),
        author_pubkey_hex: inputs.pubkey_hex.to_string(),
        version: inputs.version.to_string(),
        last_published_at: now.clone(),
        description: inputs.description.to_string(),
        tags: inputs.tags.to_vec(),
        license: inputs.license.to_string(),
    };

    match inputs.kind {
        ArtifactKind::Bundle => {
            // workspace_path = "overlays/<name>" → sidecar dir is
            // <data_dir>/overlays/<name>.
            let overlay_dir = inputs.data_dir.join(inputs.workspace_path);
            if let Err(e) = sidecar::write_sidecar(&overlay_dir, &sidecar_payload) {
                tracing::warn!(
                    error = %e,
                    overlay_dir = %overlay_dir.display(),
                    "failed to write overlay .omni-publish.json sidecar"
                );
            }
        }
        ArtifactKind::Theme => {
            // workspace_path = "themes/<filename.css>" → sidecar at
            // <data_dir>/themes/<filename.css>.publish.json.
            let themes_dir = inputs.data_dir.join("themes");
            let theme_filename = inputs
                .workspace_path
                .strip_prefix("themes/")
                .unwrap_or(inputs.workspace_path);
            if let Err(e) =
                sidecar::write_theme_sidecar(&themes_dir, theme_filename, &sidecar_payload)
            {
                tracing::warn!(
                    error = %e,
                    themes_dir = %themes_dir.display(),
                    theme_filename = theme_filename,
                    "failed to write theme sidecar"
                );
            }
        }
    }

    // Workspace-global publish-index: silent-restore source per §8.2.
    let index_path = publish_index::index_path();
    let mut idx = match publish_index::read(&index_path) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(error = %e, path = %index_path.display(), "publish-index read failed");
            return;
        }
    };
    let kind_str = match inputs.kind {
        ArtifactKind::Bundle => "overlay",
        ArtifactKind::Theme => "theme",
    };
    let entry_name: String = match inputs.kind {
        ArtifactKind::Bundle => inputs
            .workspace_path
            .strip_prefix("overlays/")
            .unwrap_or(inputs.workspace_path)
            .to_string(),
        ArtifactKind::Theme => inputs
            .workspace_path
            .strip_prefix("themes/")
            .unwrap_or(inputs.workspace_path)
            .strip_suffix(".css")
            .map(str::to_string)
            .unwrap_or_else(|| {
                inputs
                    .workspace_path
                    .strip_prefix("themes/")
                    .unwrap_or(inputs.workspace_path)
                    .to_string()
            }),
    };
    idx.upsert(PublishIndexEntry {
        pubkey_hex: inputs.pubkey_hex.to_string(),
        kind: kind_str.into(),
        name: entry_name,
        artifact_id: inputs.artifact_id.to_string(),
        last_version: inputs.version.to_string(),
        last_published_at: now,
    });
    if let Err(e) = publish_index::write(&index_path, &idx) {
        tracing::warn!(error = %e, path = %index_path.display(), "publish-index write failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Build a `PersistInputs` with the given owned `tags` Vec held by the
    /// caller. The plan originally tried to return `PersistInputs` from a
    /// helper with `tags: &["tag1".to_string()]`, but that constructs a
    /// temporary array borrowed by the returned struct — `E0515`. Threading
    /// the tags through the call site keeps every borrow rooted in the
    /// test's stack frame.
    fn inputs<'a>(
        data_dir: &'a Path,
        workspace_path: &'a str,
        kind: ArtifactKind,
        tags: &'a [String],
    ) -> PersistInputs<'a> {
        PersistInputs {
            data_dir,
            kind,
            workspace_path,
            pubkey_hex: "abcd",
            artifact_id: "ov_test",
            version: "1.0.0",
            description: "test desc",
            tags,
            license: "MIT",
        }
    }

    #[test]
    fn overlay_persist_writes_sidecar_at_expected_path() {
        let tmp = tempdir().unwrap();
        let overlay_dir = tmp.path().join("overlays").join("marathon");
        std::fs::create_dir_all(&overlay_dir).unwrap();
        let tags = vec!["tag1".to_string()];
        persist_publish_state(&inputs(
            tmp.path(),
            "overlays/marathon",
            ArtifactKind::Bundle,
            &tags,
        ));
        let sidecar = sidecar::read_sidecar(&overlay_dir).unwrap().expect("Some");
        assert_eq!(sidecar.artifact_id, "ov_test");
        assert_eq!(sidecar.author_pubkey_hex, "abcd");
        assert_eq!(sidecar.version, "1.0.0");
        assert_eq!(sidecar.description, "test desc");
        assert_eq!(sidecar.tags, vec!["tag1".to_string()]);
        assert_eq!(sidecar.license, "MIT");
        assert!(!sidecar.last_published_at.is_empty());
    }

    #[test]
    fn theme_persist_writes_flat_sidecar_at_expected_path() {
        let tmp = tempdir().unwrap();
        let themes_dir = tmp.path().join("themes");
        std::fs::create_dir_all(&themes_dir).unwrap();
        let tags = vec!["tag1".to_string()];
        persist_publish_state(&inputs(
            tmp.path(),
            "themes/synth.css",
            ArtifactKind::Theme,
            &tags,
        ));
        let sidecar = sidecar::read_theme_sidecar(&themes_dir, "synth.css")
            .unwrap()
            .expect("Some");
        assert_eq!(sidecar.artifact_id, "ov_test");
        assert_eq!(sidecar.license, "MIT");
    }

    #[test]
    fn overlay_persist_creates_overlay_dir_if_missing() {
        // Defensive: install path may target a not-yet-staged overlay folder;
        // sidecar::write_sidecar already mkdirs.
        let tmp = tempdir().unwrap();
        let tags = vec!["tag1".to_string()];
        persist_publish_state(&inputs(
            tmp.path(),
            "overlays/fresh",
            ArtifactKind::Bundle,
            &tags,
        ));
        assert!(tmp
            .path()
            .join("overlays")
            .join("fresh")
            .join(".omni-publish.json")
            .exists());
    }
}
