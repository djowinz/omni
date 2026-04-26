-- Migration 0002 — drop UNIQUE constraint on authors.display_name.
--
-- Display names are vanity-only (cosmetic) — uniqueness is enforced on
-- `pubkey` (PRIMARY KEY). Two creators must be allowed to pick the same
-- display name without one's upload failing. SQLite has no
-- `ALTER TABLE ... DROP CONSTRAINT`, so we use the standard 12-step
-- table-rebuild pattern (https://www.sqlite.org/lang_altertable.html#otheralter).
--
-- Authoritative source: umbrella spec §6.2 (this migration removes the
-- UNIQUE marker from `authors.display_name` declared in 0001).

PRAGMA foreign_keys = OFF;

CREATE TABLE authors_new (
  pubkey            BLOB PRIMARY KEY,
  display_name      TEXT,
  created_at        INTEGER NOT NULL,
  total_uploads     INTEGER NOT NULL DEFAULT 0,
  is_new_creator    INTEGER NOT NULL DEFAULT 1,
  is_denied         INTEGER NOT NULL DEFAULT 0
);

INSERT INTO authors_new (pubkey, display_name, created_at, total_uploads, is_new_creator, is_denied)
  SELECT pubkey, display_name, created_at, total_uploads, is_new_creator, is_denied FROM authors;

DROP TABLE authors;

ALTER TABLE authors_new RENAME TO authors;

PRAGMA foreign_keys = ON;
