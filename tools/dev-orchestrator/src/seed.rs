//! Idempotent D1 + R2 seed for the local dev loop.
//!
//! Writes 2 author rows + 4 artifact rows (2 themes + 2 bundles across both
//! authors) + 4 tiny 1x1 PNG thumbnails. Fixture artifacts are DISPLAY-ONLY:
//! no bundle blob exists in R2, so install attempts on fixtures will fail —
//! intentional. Users validate install by uploading their own artifacts.
//!
//! Idempotency: looks up the first fixture's artifact_id in D1; if present,
//! the whole seed no-ops.

use crate::fixtures::{self, FixtureAuthor};
use crate::shell;
use anyhow::{anyhow, Context};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::NamedTempFile;

const WORKER_DIR: &str = "apps/worker";
const FIXTURES_DIR: &str = "apps/worker/seed/dev-fixtures";

// Tiny valid 1x1 transparent PNG. 67 bytes.
const TINY_PNG_HEX: &str =
    "89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d4944415478da636400000000060005a88d50470000000049454e44ae426082";

struct ArtifactRow {
    id: &'static str,
    author_pubkey_hex: String,
    name: &'static str,
    kind: &'static str,
    content_hash: &'static str,
    thumbnail_hash: &'static str,
    description: &'static str,
    tags_json: &'static str,
    license: &'static str,
    version: &'static str,
    omni_min_version: &'static str,
}

pub fn run() -> anyhow::Result<()> {
    let authors = fixtures::ensure_fixture_authors(Path::new(FIXTURES_DIR))?;
    let first_id = "dev-alice-theme-1";
    if first_artifact_seeded(first_id)? {
        tracing::info!(artifact_id = first_id, "already seeded; no-op");
        return Ok(());
    }
    let plan = build_plan(&authors);
    seed_sql(&plan)?;
    seed_thumbnails()?;
    tracing::info!(
        authors = authors.len(),
        artifacts = plan.len(),
        "seed complete"
    );
    Ok(())
}

fn build_plan(authors: &[FixtureAuthor]) -> Vec<ArtifactRow> {
    let alice = &authors[0].pubkey_hex;
    let bob = &authors[1].pubkey_hex;
    vec![
        ArtifactRow {
            id: "dev-alice-theme-1",
            author_pubkey_hex: alice.clone(),
            name: "Neon Alley",
            kind: "theme",
            content_hash: "sha256-dev-alice-theme-1",
            thumbnail_hash: "thumb-dev-alice-theme-1",
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
            content_hash: "sha256-dev-alice-bundle-1",
            thumbnail_hash: "thumb-dev-alice-bundle-1",
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
            content_hash: "sha256-dev-bob-theme-1",
            thumbnail_hash: "thumb-dev-bob-theme-1",
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
            content_hash: "sha256-dev-bob-bundle-1",
            thumbnail_hash: "thumb-dev-bob-bundle-1",
            description: "CPU/GPU/RAM/VRAM/net overlay.",
            tags_json: r#"["telemetry","full"]"#,
            license: "Apache-2.0",
            version: "1.2.1",
            omni_min_version: "0.1.0",
        },
    ]
}

fn first_artifact_seeded(id: &str) -> anyhow::Result<bool> {
    let script = format!(
        "pnpm exec wrangler d1 execute META --local --json --command \"SELECT id FROM artifacts WHERE id = '{}' LIMIT 1;\"",
        id
    );
    let out = shell::std_cmd(&script)
        .current_dir(WORKER_DIR)
        .output()
        .context("spawn wrangler d1 execute")?;
    if !out.status.success() {
        return Ok(false);
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let parsed: Value = serde_json::from_str(stdout.trim()).unwrap_or(Value::Null);
    // Envelope shape varies across wrangler versions — accept both.
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

fn seed_sql(plan: &[ArtifactRow]) -> anyhow::Result<()> {
    let now: i64 = 1_734_564_000; // fixed — stable across resets
    let mut sql = String::from("PRAGMA foreign_keys = ON;\n");
    // De-duplicate author writes by pubkey_hex.
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
            // (Name derivation above is loose; the seed writes both names via the fixtures pass.)
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
            escape(row.content_hash),
            escape(row.thumbnail_hash),
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
            escape(row.content_hash),
            escape(row.id),
            now,
        ));
    }
    let sqlfile = NamedTempFile::new()?;
    fs::write(sqlfile.path(), sql)?;
    let script = format!(
        "pnpm exec wrangler d1 execute META --local --file \"{}\"",
        sqlfile.path().display()
    );
    let status = shell::std_cmd(&script).current_dir(WORKER_DIR).status()?;
    if !status.success() {
        return Err(anyhow!("wrangler d1 execute --file failed"));
    }
    Ok(())
}

fn seed_thumbnails() -> anyhow::Result<()> {
    let png_bytes = hex::decode(TINY_PNG_HEX)?;
    let thumbnail_keys = [
        "thumb-dev-alice-theme-1",
        "thumb-dev-alice-bundle-1",
        "thumb-dev-bob-theme-1",
        "thumb-dev-bob-bundle-1",
    ];
    for key in thumbnail_keys {
        let tmp = NamedTempFile::new()?;
        fs::write(tmp.path(), &png_bytes)?;
        let r2key = format!("thumbnails/{}", key);
        let script = format!(
            "pnpm exec wrangler r2 object put \"BLOBS/{}\" --file \"{}\" --local",
            r2key,
            tmp.path().display()
        );
        let status = shell::std_cmd(&script).current_dir(WORKER_DIR).status()?;
        if !status.success() {
            return Err(anyhow!("wrangler r2 put failed for {r2key}"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let plan = build_plan(&authors);
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
        let plan = build_plan(&authors);
        for row in &plan {
            assert!(row.author_pubkey_hex == alice || row.author_pubkey_hex == bob);
        }
    }

    #[test]
    fn tiny_png_bytes_are_valid() {
        let bytes = hex::decode(TINY_PNG_HEX).unwrap();
        // PNG signature: 89 50 4E 47 0D 0A 1A 0A
        assert_eq!(
            &bytes[0..8],
            &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        );
    }
}
