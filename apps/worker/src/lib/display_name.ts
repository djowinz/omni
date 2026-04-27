/**
 * Shared `display_name` validation per
 * `2026-04-26-identity-completion-and-display-name-design.md` §3.4.
 *
 * Single worker-side source of truth for the rule set so that
 * `PUT /v1/author/me` (`routes/author.ts`) and `POST /v1/upload`'s optional
 * `display_name` form field (`routes/upload.ts`) agree byte-for-byte with
 * each other AND with the host-side `identity.setDisplayName` validation
 * over in `crates/host/`. Extracting the helper here is plan §T5 step 5.3 —
 * the pre-T5 inline copy in `routes/author.ts` was the only worker-side
 * implementation; both call sites import this module now.
 *
 * Rules (verbatim from §3.4):
 *   1. NFC normalize.
 *   2. Trim leading/trailing whitespace.
 *   3. Reject unless length (after NFC + trim) is in 1..=32.
 *   4. Reject any `\p{Cc}` (control) or `\p{Cs}` (surrogate) code point.
 *   5. Accept everything else (Unicode generously allowed; emoji legal).
 *   6. No banlist v1.
 *
 * Length is measured in **Unicode code points** (NFC scalar values), NOT
 * UTF-16 code units, so the worker matches the host-side Rust validator
 * (`s.chars().count()`). An astral-plane emoji like 😀 (U+1F600) counts
 * as 1 code point, not 2 UTF-16 code units. Spread-into-array
 * (`[...normalized].length`) iterates the string by code points via the
 * String iterator protocol, giving the correct count. This pin matches
 * spec §3.4 (revised 2026-04-27 after T4 quality review I1 caught the
 * implicit unit drift between worker and host validators).
 */

const DISPLAY_NAME_MAX = 32;

/**
 * Apply §3.4 rules to a display_name candidate. Returns either the
 * normalized accepted form (`{ ok }`) or a structured rejection
 * (`{ err }`) whose message is surfaced verbatim in the BadRequest
 * envelope so debuggability survives the wire.
 */
export function validateDisplayName(raw: unknown): { ok: string } | { err: string } {
  if (typeof raw !== 'string') return { err: 'display_name must be a string' };
  const normalized = raw.normalize('NFC').trim();
  // Code-point length, matching host Rust `s.chars().count()`. The spread
  // operator drives the String iterator protocol, which yields one entry
  // per Unicode scalar value (handling surrogate pairs correctly).
  const codePointLength = [...normalized].length;
  if (codePointLength === 0 || codePointLength > DISPLAY_NAME_MAX) {
    return { err: 'display_name must be 1-32 characters after trim' };
  }
  // Iterate code points (`for...of` on a string yields code points, handling
  // surrogate pairs). `\p{Cc}` is U+0000..U+001F and U+007F..U+009F;
  // `\p{Cs}` is U+D800..U+DFFF (paired surrogates can't appear in a
  // well-formed UTF-16 string here, but unpaired ones could; reject them
  // defensively).
  for (const ch of normalized) {
    const cp = ch.codePointAt(0)!;
    if ((cp >= 0x00 && cp <= 0x1f) || (cp >= 0x7f && cp <= 0x9f)) {
      return { err: 'display_name contains control characters' };
    }
    if (cp >= 0xd800 && cp <= 0xdfff) {
      return { err: 'display_name contains surrogate code points' };
    }
  }
  return { ok: normalized };
}
