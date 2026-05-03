/**
 * publish-sidecar-schema.spec.ts — Wave 2 / Issue I2 backward-compat regression.
 *
 * The Zod schema for `.omni-publish.json` (PublishSidecarSchema) was extended
 * in Wave 1 / Task 1 with three new optional fields — `description`, `tags`,
 * `license` — that cache the manifest fields the upload dialog's Step 2 form
 * prefills from on update mode (INV-7.5.3). The schema decorates each new
 * field with `.default('')` / `.default([])` so sidecars written by older
 * host versions (pre-INV-7.5.3) still parse cleanly with empty defaults,
 * matching the host's `serde(default)` semantics on
 * `crates/host/src/share/sidecar.rs::PublishSidecar`.
 *
 * This file pins both the backward-compat default behavior and the
 * verbatim-passthrough behavior for new-shape sidecars, so a future schema
 * edit that drops the `.default(...)` decoration breaks at test-time
 * instead of at runtime when an old sidecar fails to parse.
 */

import { describe, it, expect } from 'vitest';
import { PublishSidecarSchema } from '../share-types';

describe('PublishSidecarSchema — backward compat (Issue I2)', () => {
  it('parses an old-shape sidecar (pre-INV-7.5.3) with empty defaults for new fields', () => {
    // Sidecars written before the description/tags/license expansion (Wave 1
    // T1) omit the new fields. The .default() decorations on the schema
    // must backfill empty values so Zod parsing doesn't fail and the
    // prefill effect sees stable empty primitives rather than `undefined`.
    const oldShape = {
      artifact_id: 'ov_legacy',
      author_pubkey_hex: 'abcd',
      version: '0.1.0',
      last_published_at: '2026-04-18T00:00:00Z',
    };
    const parsed = PublishSidecarSchema.parse(oldShape);
    expect(parsed.artifact_id).toBe('ov_legacy');
    expect(parsed.author_pubkey_hex).toBe('abcd');
    expect(parsed.version).toBe('0.1.0');
    expect(parsed.last_published_at).toBe('2026-04-18T00:00:00Z');
    expect(parsed.description).toBe('');
    expect(parsed.tags).toEqual([]);
    expect(parsed.license).toBe('');
  });

  it('preserves new-shape sidecar fields verbatim', () => {
    const newShape = {
      artifact_id: 'ov_new',
      author_pubkey_hex: 'abcd',
      version: '1.3.0',
      last_published_at: '2026-04-18T00:00:00Z',
      description: 'desc',
      tags: ['a', 'b'],
      license: 'MIT',
    };
    const parsed = PublishSidecarSchema.parse(newShape);
    expect(parsed.description).toBe('desc');
    expect(parsed.tags).toEqual(['a', 'b']);
    expect(parsed.license).toBe('MIT');
  });
});
