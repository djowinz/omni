/**
 * Unit tests for `isThemeOnly` (apps/worker/src/routes/upload.ts).
 *
 * Spec §8.7 / Plan Task A0.5-6:
 *   Pre-2026-04-25, the function returned `true` on missing/null/empty
 *   `resource_kinds`, silently biasing every host-uploaded bundle that hadn't
 *   populated the field into the lighter theme rate-limit bucket. The fix
 *   inverts those branches — absence is NOT theme-only-by-default; the host
 *   (OWI-33 / Task A0.7) populates `resource_kinds` so legitimate non-theme
 *   bundles classify correctly.
 *
 * OWI-59 / Plan Task A1.7:
 *   `serde-wasm-bindgen` (used by the omni-bundle WASM in
 *   `apps/worker/src/lib/wasm.ts`) serializes Rust `BTreeMap<String, _>` as a
 *   JS `Map`, NOT a plain object. `Object.keys(map)` returns `[]` for a Map,
 *   so the post-OWI-32 implementation misclassifies every real upload as
 *   not-theme-only. Handle both Map and plain-object inputs.
 */
import { describe, it, expect } from 'vitest';
import { isThemeOnly } from '../src/routes/upload';

describe('isThemeOnly', () => {
  // ---- Object inputs (serialized via JSON / plain JS) ----------------------

  it('returns false when resource_kinds is missing', () => {
    expect(isThemeOnly({} as never)).toBe(false);
  });

  it('returns false when resource_kinds is null', () => {
    expect(isThemeOnly({ resource_kinds: null } as never)).toBe(false);
  });

  it('returns false when resource_kinds is an empty object', () => {
    expect(isThemeOnly({ resource_kinds: {} } as never)).toBe(false);
  });

  it('returns true when resource_kinds is theme-only', () => {
    expect(isThemeOnly({ resource_kinds: { theme: 1 } } as never)).toBe(true);
  });

  it('returns false when resource_kinds includes a non-theme key', () => {
    expect(isThemeOnly({ resource_kinds: { theme: 1, font: 2 } } as never)).toBe(false);
  });

  // ---- Map inputs (serde-wasm-bindgen serializes BTreeMap as Map) ---------

  it('returns true when resource_kinds is a Map containing only "theme"', () => {
    const kinds = new Map<string, unknown>([['theme', 1]]);
    expect(isThemeOnly({ resource_kinds: kinds } as never)).toBe(true);
  });

  it('returns false when resource_kinds is a Map containing theme + font', () => {
    const kinds = new Map<string, unknown>([
      ['theme', 1],
      ['font', 2],
    ]);
    expect(isThemeOnly({ resource_kinds: kinds } as never)).toBe(false);
  });

  it('returns false when resource_kinds is an empty Map', () => {
    const kinds = new Map<string, unknown>();
    expect(isThemeOnly({ resource_kinds: kinds } as never)).toBe(false);
  });
});
