import { describe, it, expect, beforeEach } from 'vitest';
import { env } from 'cloudflare:test';

// The workerd-based test pool does not implement `node:fs`, so we load the
// migration SQL via Vite's `?raw` asset transform at build time. Same
// pattern used by `canonical_parity.test.ts`. TypeScript doesn't know
// about the `?raw` suffix without an ambient declaration; the
// ts-expect-error directive below pins the suppression to those two lines.
// @ts-expect-error — `?raw` is a Vite import suffix handled at transform time.
import migration0001Sql from '../migrations/0001_initial_schema.sql?raw';
// @ts-expect-error — `?raw` is a Vite import suffix handled at transform time.
import migration0002Sql from '../migrations/0002_drop_authors_display_name_unique.sql?raw';

/**
 * Strip SQL line comments (`-- ...` to EOL) and split the remaining text
 * into individual statements. D1's `exec` only handles single-line
 * statements (newline-separated); multi-line CREATE TABLEs require
 * `prepare(...).run()` instead. We use prepare here so the canonical
 * formatted SQL files (multi-line, comment-rich) work directly.
 */
function splitStatements(sql: string): string[] {
  const stripped = sql
    .split('\n')
    .map((line) => {
      const idx = line.indexOf('--');
      return idx >= 0 ? line.slice(0, idx) : line;
    })
    .join('\n');
  return stripped
    .split(';')
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

async function runMigration(sql: string): Promise<void> {
  for (const stmt of splitStatements(sql)) {
    await env.META.prepare(stmt).run();
  }
}

describe('migration 0002 — drop authors.display_name UNIQUE', () => {
  beforeEach(async () => {
    // Apply migration 0001 to start from baseline.
    await runMigration(migration0001Sql);
  });

  it('removes UNIQUE constraint on display_name', async () => {
    const seedNow = 1714000000;
    await env.META.prepare(
      'INSERT INTO authors (pubkey, display_name, created_at) VALUES (?, ?, ?)',
    )
      .bind(new Uint8Array([1, 2, 3]), 'starfire', seedNow)
      .run();

    // Sanity — current schema must reject duplicate.
    await expect(
      env.META.prepare('INSERT INTO authors (pubkey, display_name, created_at) VALUES (?, ?, ?)')
        .bind(new Uint8Array([4, 5, 6]), 'starfire', seedNow)
        .run(),
    ).rejects.toThrow();

    // Apply migration 0002.
    await runMigration(migration0002Sql);

    // Post-migration: duplicate display_name now allowed.
    await env.META.prepare(
      'INSERT INTO authors (pubkey, display_name, created_at) VALUES (?, ?, ?)',
    )
      .bind(new Uint8Array([4, 5, 6]), 'starfire', seedNow)
      .run();

    const rows = await env.META.prepare('SELECT COUNT(*) as n FROM authors WHERE display_name = ?')
      .bind('starfire')
      .first<{ n: number }>();
    expect(rows?.n).toBe(2);

    // And the original row survived the migration.
    const original = await env.META.prepare(
      'SELECT pubkey FROM authors WHERE display_name = ? ORDER BY created_at LIMIT 1',
    )
      .bind('starfire')
      .first<{ pubkey: ArrayBuffer }>();
    expect(new Uint8Array(original!.pubkey)[0]).toBe(1);
  });
});
