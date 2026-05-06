//! Idempotent D1 + R2 seed for the local dev loop.
//!
//! Writes 2 author rows + 4 artifact rows (2 themes + 2 bundles across both
//! authors), 4 tiny 1x1 PNG thumbnails, AND 4 real `.omnipkg` bundle blobs in
//! R2 keyed by their canonical content_hash. Fixture artifacts are
//! end-to-end installable: `explorer.install` from the Discover tab will
//! download → unpack-worker-attested → sanitize → stage → commit.
//!
//! Bundle shape per fixture is intentionally minimal so they pass the
//! sanitize gate without depending on any ambient state — the seeder
//! shouldn't need to track sanitize-rule churn. Each bundle gets a
//! distinguishable `manifest.name` + a unique `description` so canonical
//! hashes are guaranteed distinct (the canonical_hash is sha256-of-manifest;
//! identical manifests would collide on the same R2 key).
//!
//! Idempotency: looks up the first fixture's artifact_id in D1; if present,
//! the whole seed no-ops. To re-seed with regenerated bundles after changing
//! this file, run `omni-dev reset` first.

use crate::fixtures::{self, FixtureAuthor};
use crate::shell;
use anyhow::{anyhow, Context};
use bundle::{canonical_hash, pack, BundleLimits, FileEntry, Manifest, Tag};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use tempfile::NamedTempFile;

const WORKER_DIR: &str = "apps/worker";
const FIXTURES_DIR: &str = "apps/worker/seed/dev-fixtures";

// R2 bucket name (NOT the wrangler binding alias). The wrangler.toml binding
// `BLOBS` points at this bucket; the wrangler r2 CLI takes the real bucket
// name + the relative key. Same bucket holds both `thumbnails/` and
// `bundles/` keys per the worker's GET routes.
const R2_BUCKET: &str = "omni-themes-blobs";

// Tiny valid 1x1 transparent PNG. 67 bytes.
const TINY_PNG_HEX: &str =
    "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d4944415478da636400000000060005a88d50470000000049454e44ae426082";

/// What a single fixture artifact ends up as: the D1 row, plus the bundle
/// bytes that get uploaded to R2 at `bundles/<content_hash>.omnipkg`. The
/// `content_hash` field is the canonical-hash hex computed from the bundle's
/// own manifest — NOT a placeholder string like the pre-real-bundles seed
/// used. (That earlier shape made fixtures display-only — install attempts
/// would 500 because no R2 blob existed.)
struct ArtifactRow {
    id: &'static str,
    author_pubkey_hex: String,
    name: &'static str,
    kind: &'static str,
    content_hash: String,
    bundle_bytes: Vec<u8>,
    description: &'static str,
    tags_json: &'static str,
    license: &'static str,
    version: &'static str,
    omni_min_version: &'static str,
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

fn tiny_png_thumbnail_hash() -> anyhow::Result<String> {
    let bytes = hex::decode(TINY_PNG_HEX)?;
    Ok(hex::encode(sha256(&bytes)))
}

/// Build a minimal sanitize-clean theme bundle: stub overlay + a tagged CSS
/// file. The CSS body is intentionally trivial; sanitize's CSS rules can
/// shift over time and we'd rather not chase them across seed re-runs.
fn build_theme_bundle(
    name: &str,
    version: &str,
    description: &str,
    tags: &[&str],
    license: &str,
    css_marker: &str,
) -> anyhow::Result<(Vec<u8>, String)> {
    let stub_overlay = format!(
        r#"<widget id="root" name="{name}"><template><div></div></template><style></style></widget>"#
    )
    .into_bytes();
    // Distinct CSS body per theme so two themes don't collide on canonical
    // hash. The `--marker` custom property is harmless and just changes the
    // file's bytes (and therefore its sha256 in the manifest, which feeds
    // the canonical hash).
    let css = format!(":root {{ --omni-fixture: {css_marker}; }}\n").into_bytes();

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert("overlay.omni".into(), stub_overlay.clone());
    files.insert("themes/default.css".into(), css.clone());

    let manifest = Manifest {
        schema_version: 1,
        name: name.into(),
        version: version
            .parse()
            .with_context(|| format!("bad version `{version}`"))?,
        omni_min_version: "0.1.0".parse().unwrap(),
        description: description.into(),
        tags: tags
            .iter()
            .map(|t| Tag::new(*t))
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| "tag validation failed")?,
        license: license.into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: Some("themes/default.css".into()),
        sensor_requirements: vec![],
        files: vec![
            FileEntry {
                path: "overlay.omni".into(),
                sha256: sha256(&stub_overlay),
            },
            FileEntry {
                path: "themes/default.css".into(),
                sha256: sha256(&css),
            },
        ],
        resource_kinds: None,
    };

    let bytes = pack(&manifest, &files, &BundleLimits::DEFAULT)
        .map_err(|e| anyhow!("pack theme `{name}`: {e}"))?;
    let hash_hex = hex::encode(canonical_hash(&manifest, &files));
    Ok((bytes, hash_hex))
}

/// Build a minimal overlay bundle with a single `overlay.omni` file. No
/// theme reference (default_theme = None), no extra assets.
fn build_overlay_bundle(
    name: &str,
    version: &str,
    description: &str,
    tags: &[&str],
    license: &str,
    omni_marker: &str,
) -> anyhow::Result<(Vec<u8>, String)> {
    // Marker varies the file bytes per overlay so the canonical hash is
    // unique even when other manifest fields collide.
    let omni = format!(
        r#"<widget id="root" name="{name}"><template><div data-fixture="{omni_marker}">{name}</div></template><style></style></widget>"#
    )
    .into_bytes();

    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    files.insert("overlay.omni".into(), omni.clone());

    let manifest = Manifest {
        schema_version: 1,
        name: name.into(),
        version: version
            .parse()
            .with_context(|| format!("bad version `{version}`"))?,
        omni_min_version: "0.1.0".parse().unwrap(),
        description: description.into(),
        tags: tags
            .iter()
            .map(|t| Tag::new(*t))
            .collect::<Result<Vec<_>, _>>()
            .with_context(|| "tag validation failed")?,
        license: license.into(),
        entry_overlay: "overlay.omni".into(),
        default_theme: None,
        sensor_requirements: vec![],
        files: vec![FileEntry {
            path: "overlay.omni".into(),
            sha256: sha256(&omni),
        }],
        resource_kinds: None,
    };

    let bytes = pack(&manifest, &files, &BundleLimits::DEFAULT)
        .map_err(|e| anyhow!("pack overlay `{name}`: {e}"))?;
    let hash_hex = hex::encode(canonical_hash(&manifest, &files));
    Ok((bytes, hash_hex))
}

pub fn run() -> anyhow::Result<()> {
    let authors = fixtures::ensure_fixture_authors(Path::new(FIXTURES_DIR))?;
    let first_id = "dev-alice-theme-1";
    if first_artifact_seeded(first_id)? {
        tracing::info!(artifact_id = first_id, "already seeded; no-op");
        return Ok(());
    }
    let plan = build_plan(&authors)?;
    let thumbnail_hash = tiny_png_thumbnail_hash()?;
    seed_sql(&plan, &thumbnail_hash)?;
    seed_thumbnails(&thumbnail_hash)?;
    seed_bundles(&plan)?;
    tracing::info!(
        authors = authors.len(),
        artifacts = plan.len(),
        thumbnail_hash = %thumbnail_hash,
        "seed complete"
    );
    Ok(())
}

fn build_plan(authors: &[FixtureAuthor]) -> anyhow::Result<Vec<ArtifactRow>> {
    let alice = &authors[0].pubkey_hex;
    let bob = &authors[1].pubkey_hex;

    let (alice_theme_bytes, alice_theme_hash) = build_theme_bundle(
        "Neon Alley",
        "1.0.0",
        "Cyan-pink neon with scanline overlay.",
        &["dark", "neon"],
        "MIT",
        "neon-alley",
    )?;
    let (alice_bundle_bytes, alice_bundle_hash) = build_overlay_bundle(
        "HWMon Compact",
        "1.0.0",
        "Compact FPS + temps overlay.",
        &["fps", "temps", "compact"],
        "MIT",
        "hwmon-compact",
    )?;
    let (bob_theme_bytes, bob_theme_hash) = build_theme_bundle(
        "Solarize Lite",
        "2.3.0",
        "Low-contrast solarized palette.",
        &["light", "minimal"],
        "Apache-2.0",
        "solarize-lite",
    )?;
    let (bob_bundle_bytes, bob_bundle_hash) = build_overlay_bundle(
        "Full Telemetry",
        "1.2.1",
        "CPU/GPU/RAM/VRAM/net overlay.",
        &["telemetry", "full"],
        "Apache-2.0",
        "full-telemetry",
    )?;

    Ok(vec![
        ArtifactRow {
            id: "dev-alice-theme-1",
            author_pubkey_hex: alice.clone(),
            name: "Neon Alley",
            kind: "theme",
            content_hash: alice_theme_hash,
            bundle_bytes: alice_theme_bytes,
            description: "Cyan-pink neon with scanline overlay.",
            tags_json: r#"["dark","neon"]"#,
            license: "MIT",
            version: "1.0.0",
            omni_min_version: "0.1.0",
        },
        ArtifactRow {
            id: "dev-alice-bundle-1",
            author_pubkey_hex: alice.clone(),
            name: "HWMon Compact",
            kind: "bundle",
            content_hash: alice_bundle_hash,
            bundle_bytes: alice_bundle_bytes,
            description: "Compact FPS + temps overlay.",
            tags_json: r#"["fps","temps","compact"]"#,
            license: "MIT",
            version: "1.0.0",
            omni_min_version: "0.1.0",
        },
        ArtifactRow {
            id: "dev-bob-theme-1",
            author_pubkey_hex: bob.clone(),
            name: "Solarize Lite",
            kind: "theme",
            content_hash: bob_theme_hash,
            bundle_bytes: bob_theme_bytes,
            description: "Low-contrast solarized palette.",
            tags_json: r#"["light","minimal"]"#,
            license: "Apache-2.0",
            version: "2.3.0",
            omni_min_version: "0.1.0",
        },
        ArtifactRow {
            id: "dev-bob-bundle-1",
            author_pubkey_hex: bob.clone(),
            name: "Full Telemetry",
            kind: "bundle",
            content_hash: bob_bundle_hash,
            bundle_bytes: bob_bundle_bytes,
            description: "CPU/GPU/RAM/VRAM/net overlay.",
            tags_json: r#"["telemetry","full"]"#,
            license: "Apache-2.0",
            version: "1.2.1",
            omni_min_version: "0.1.0",
        },
    ])
}

fn first_artifact_seeded(id: &str) -> anyhow::Result<bool> {
    let sql = format!("SELECT id FROM artifacts WHERE id = '{}' LIMIT 1;", id);
    let out = shell::std_cmd(
        "pnpm",
        [
            "exec",
            "wrangler",
            "d1",
            "execute",
            "META",
            "--local",
            "--json",
            "--command",
            &sql,
        ],
    )
    .current_dir(WORKER_DIR)
    .output()
    .context("spawn wrangler d1 execute")?;
    if !out.status.success() {
        return Ok(false);
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: Value = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
    let rows = if let Some(arr) = parsed.as_array() {
        arr.first()
            .and_then(|o| o.get("results"))
            .and_then(|r| r.as_array())
            .cloned()
    } else {
        parsed.get("results").and_then(|r| r.as_array()).cloned()
    };
    Ok(rows.map(|r| !r.is_empty()).unwrap_or(false))
}

fn seed_sql(plan: &[ArtifactRow], thumbnail_hash: &str) -> anyhow::Result<()> {
    let now: i64 = 1_734_564_000;
    let mut sql = String::from("PRAGMA foreign_keys = ON;\n");
    let mut seen_authors: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for row in plan {
        if seen_authors.insert(row.author_pubkey_hex.as_str()) {
            let display = if row
                .author_pubkey_hex
                .starts_with(plan[0].author_pubkey_hex.as_str())
            {
                "dev-alice"
            } else {
                "dev-bob"
            };
            sql.push_str(&format!(
                "INSERT OR IGNORE INTO authors (pubkey, display_name, created_at, total_uploads, is_new_creator, is_denied) \
                 VALUES (X'{}', '{}', {}, 0, 1, 0);\n",
                row.author_pubkey_hex, display, now
            ));
        }
    }
    for row in plan {
        let escape = |s: &str| s.replace('\'', "''");
        sql.push_str(&format!(
            "INSERT OR IGNORE INTO artifacts \
               (id, author_pubkey, name, kind, content_hash, thumbnail_hash, description, tags, \
                license, version, omni_min_version, signature, created_at, updated_at, \
                install_count, report_count, is_removed, is_featured) \
             VALUES ('{}', X'{}', '{}', '{}', '{}', '{}', '{}', '{}', \
                     '{}', '{}', '{}', X'{}', {}, {}, 0, 0, 0, 0);\n",
            escape(row.id),
            row.author_pubkey_hex,
            escape(row.name),
            escape(row.kind),
            escape(&row.content_hash),
            escape(thumbnail_hash),
            escape(row.description),
            escape(row.tags_json),
            escape(row.license),
            escape(row.version),
            escape(row.omni_min_version),
            "00".repeat(64),
            now,
            now,
        ));
        sql.push_str(&format!(
            "INSERT OR IGNORE INTO content_hashes (content_hash, artifact_id, first_seen_at) \
             VALUES ('{}', '{}', {});\n",
            escape(&row.content_hash),
            escape(row.id),
            now,
        ));
    }
    let sqlfile = NamedTempFile::new()?;
    fs::write(sqlfile.path(), sql)?;
    let sqlfile_path = sqlfile.path().to_string_lossy().to_string();
    let status = shell::std_cmd(
        "pnpm",
        [
            "exec",
            "wrangler",
            "d1",
            "execute",
            "META",
            "--local",
            "--file",
            &sqlfile_path,
        ],
    )
    .current_dir(WORKER_DIR)
    .status()?;
    if !status.success() {
        return Err(anyhow!("wrangler d1 execute --file failed"));
    }
    Ok(())
}

fn seed_thumbnails(thumbnail_hash: &str) -> anyhow::Result<()> {
    let png_bytes = hex::decode(TINY_PNG_HEX)?;
    let tmp = NamedTempFile::new()?;
    fs::write(tmp.path(), &png_bytes)?;
    let r2_path = format!("{R2_BUCKET}/thumbnails/{}.png", thumbnail_hash);
    let tmp_path = tmp.path().to_string_lossy().to_string();
    let status = shell::std_cmd(
        "pnpm",
        [
            "exec", "wrangler", "r2", "object", "put", &r2_path, "--file", &tmp_path, "--local",
        ],
    )
    .current_dir(WORKER_DIR)
    .status()?;
    if !status.success() {
        return Err(anyhow!("wrangler r2 put failed for {r2_path}"));
    }
    Ok(())
}

/// Upload each fixture's `.omnipkg` bytes to R2 at the canonical key the
/// download route reads (`bundles/<content_hash>.omnipkg`). Bundles with the
/// same canonical hash collide on the same key — that's fine, the second put
/// is a no-op replace. In practice each fixture's manifest is distinguishable
/// (different name + description) so each gets a unique key.
fn seed_bundles(plan: &[ArtifactRow]) -> anyhow::Result<()> {
    for row in plan {
        let tmp = NamedTempFile::new()?;
        fs::write(tmp.path(), &row.bundle_bytes)?;
        let r2_path = format!("{R2_BUCKET}/bundles/{}.omnipkg", row.content_hash);
        let tmp_path = tmp.path().to_string_lossy().to_string();
        let status = shell::std_cmd(
            "pnpm",
            [
                "exec", "wrangler", "r2", "object", "put", &r2_path, "--file", &tmp_path,
                "--local",
            ],
        )
        .current_dir(WORKER_DIR)
        .status()?;
        if !status.success() {
            return Err(anyhow!("wrangler r2 put failed for {r2_path}"));
        }
        tracing::info!(
            artifact_id = row.id,
            content_hash = %row.content_hash,
            bytes = row.bundle_bytes.len(),
            "uploaded fixture bundle"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bundle::{unpack, BundleLimits};

    #[test]
    fn build_plan_produces_2_themes_and_2_bundles() {
        let authors = vec![
            FixtureAuthor {
                slug: "alice".into(),
                display_name: "dev-alice".into(),
                pubkey_hex: "aa".repeat(32),
            },
            FixtureAuthor {
                slug: "bob".into(),
                display_name: "dev-bob".into(),
                pubkey_hex: "bb".repeat(32),
            },
        ];
        let plan = build_plan(&authors).unwrap();
        assert_eq!(plan.len(), 4);
        let kinds: Vec<&str> = plan.iter().map(|r| r.kind).collect();
        assert_eq!(kinds.iter().filter(|k| **k == "theme").count(), 2);
        assert_eq!(kinds.iter().filter(|k| **k == "bundle").count(), 2);
    }

    #[test]
    fn build_plan_stamps_author_pubkey_on_each_artifact() {
        let alice = "aa".repeat(32);
        let bob = "bb".repeat(32);
        let authors = vec![
            FixtureAuthor {
                slug: "alice".into(),
                display_name: "dev-alice".into(),
                pubkey_hex: alice.clone(),
            },
            FixtureAuthor {
                slug: "bob".into(),
                display_name: "dev-bob".into(),
                pubkey_hex: bob.clone(),
            },
        ];
        let plan = build_plan(&authors).unwrap();
        for row in &plan {
            assert!(row.author_pubkey_hex == alice || row.author_pubkey_hex == bob);
        }
    }

    #[test]
    fn tiny_png_bytes_are_valid() {
        let bytes = hex::decode(TINY_PNG_HEX).unwrap();
        assert_eq!(
            &bytes[0..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        );
    }

    #[test]
    fn tiny_png_thumbnail_hash_is_64_hex_chars() {
        let hash = tiny_png_thumbnail_hash().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()));
    }

    #[test]
    fn tiny_png_thumbnail_hash_is_deterministic() {
        let h1 = tiny_png_thumbnail_hash().unwrap();
        let h2 = tiny_png_thumbnail_hash().unwrap();
        assert_eq!(h1, h2);
    }

    #[test]
    fn each_fixture_has_distinct_content_hash() {
        // Regression for "two themes collide on R2 key" — manifests must
        // differ on enough fields (name, description) to produce unique
        // canonical hashes. Otherwise two artifacts would point at the
        // same R2 blob and uninstalling one would orphan the other's row.
        let authors = vec![
            FixtureAuthor {
                slug: "alice".into(),
                display_name: "dev-alice".into(),
                pubkey_hex: "aa".repeat(32),
            },
            FixtureAuthor {
                slug: "bob".into(),
                display_name: "dev-bob".into(),
                pubkey_hex: "bb".repeat(32),
            },
        ];
        let plan = build_plan(&authors).unwrap();
        let mut hashes: Vec<&String> = plan.iter().map(|r| &r.content_hash).collect();
        hashes.sort();
        let original_len = hashes.len();
        hashes.dedup();
        assert_eq!(
            hashes.len(),
            original_len,
            "fixture content_hashes must be unique"
        );
    }

    fn unpack_to_map(bytes: &[u8]) -> (bundle::Manifest, BTreeMap<String, Vec<u8>>) {
        let unpacked = unpack(bytes, &BundleLimits::DEFAULT).expect("unpack");
        unpacked.into_map().expect("into_map")
    }

    #[test]
    fn each_fixture_bundle_is_a_valid_omnipkg() {
        // Round-trip: pack → unpack must succeed, manifest fields preserved,
        // file sha256 entries match actual file bytes. If sanitize ever
        // tightens enough to reject our minimal stub, this fails first
        // (clear signal: the seed needs updating, NOT the production code).
        let authors = vec![
            FixtureAuthor {
                slug: "alice".into(),
                display_name: "dev-alice".into(),
                pubkey_hex: "aa".repeat(32),
            },
            FixtureAuthor {
                slug: "bob".into(),
                display_name: "dev-bob".into(),
                pubkey_hex: "bb".repeat(32),
            },
        ];
        let plan = build_plan(&authors).unwrap();
        for row in &plan {
            let (manifest, _files) = unpack_to_map(&row.bundle_bytes);
            assert_eq!(manifest.name, row.name, "name mismatch on {}", row.id);
            assert_eq!(
                manifest.version.to_string(),
                row.version,
                "version mismatch on {}",
                row.id
            );
        }
    }

    #[test]
    fn each_fixture_bundle_is_sanitize_clean() {
        // Sanitize gates install — if our seeded bundles trip it,
        // `explorer.install` would fail end-to-end. This catches the
        // regression at seed-build time so we don't ship a bundle that
        // can be downloaded but never installed.
        let authors = vec![
            FixtureAuthor {
                slug: "alice".into(),
                display_name: "dev-alice".into(),
                pubkey_hex: "aa".repeat(32),
            },
            FixtureAuthor {
                slug: "bob".into(),
                display_name: "dev-bob".into(),
                pubkey_hex: "bb".repeat(32),
            },
        ];
        let plan = build_plan(&authors).unwrap();
        for row in &plan {
            let (manifest, files) = unpack_to_map(&row.bundle_bytes);
            sanitize::sanitize_bundle(&manifest, files)
                .unwrap_or_else(|e| panic!("sanitize rejected `{}`: {e}", row.id));
        }
    }
}
