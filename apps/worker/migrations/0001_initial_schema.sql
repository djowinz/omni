-- Omni themes worker — initial schema.
-- Authoritative source: umbrella spec §6.2. Keep in sync on every umbrella change.

PRAGMA foreign_keys = ON;

-- `display_name` is intentionally NOT UNIQUE — uniqueness is enforced on
-- `pubkey` (PRIMARY KEY). Display names are vanity-only / cosmetic; two
-- creators must be allowed to pick the same name without one's upload
-- failing. The renderer disambiguates by appending the fingerprint suffix
-- (e.g. `djowinz#abc12345`). Authoritative source: umbrella spec §6.2.
CREATE TABLE authors (
  pubkey            BLOB PRIMARY KEY,
  display_name      TEXT,
  created_at        INTEGER NOT NULL,
  total_uploads     INTEGER NOT NULL DEFAULT 0,
  is_new_creator    INTEGER NOT NULL DEFAULT 1,
  is_denied         INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE artifacts (
  id                TEXT PRIMARY KEY,
  author_pubkey     BLOB NOT NULL REFERENCES authors(pubkey),
  name              TEXT NOT NULL,
  kind              TEXT NOT NULL,
  content_hash      TEXT NOT NULL,
  thumbnail_hash    TEXT NOT NULL,
  description       TEXT,
  tags              TEXT,
  license           TEXT,
  version           TEXT NOT NULL,
  omni_min_version  TEXT NOT NULL,
  signature         BLOB NOT NULL,
  created_at        INTEGER NOT NULL,
  updated_at        INTEGER NOT NULL,
  install_count     INTEGER NOT NULL DEFAULT 0,
  report_count      INTEGER NOT NULL DEFAULT 0,
  is_removed        INTEGER NOT NULL DEFAULT 0,
  is_featured       INTEGER NOT NULL DEFAULT 0,
  UNIQUE (author_pubkey, name)
);

CREATE TABLE content_hashes (
  content_hash      TEXT PRIMARY KEY,
  artifact_id       TEXT NOT NULL REFERENCES artifacts(id),
  first_seen_at     INTEGER NOT NULL
);

CREATE TABLE tombstones (
  content_hash      TEXT PRIMARY KEY,
  reason            TEXT,
  removed_at        INTEGER NOT NULL
);

CREATE TABLE install_daily (
  artifact_id       TEXT NOT NULL,
  day               TEXT NOT NULL,
  install_count     INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (artifact_id, day)
);

CREATE INDEX idx_artifacts_kind_created ON artifacts(kind, created_at DESC);
CREATE INDEX idx_artifacts_tags         ON artifacts(tags);
CREATE INDEX idx_artifacts_author       ON artifacts(author_pubkey);
CREATE INDEX idx_install_daily_day      ON install_daily(day);
