/**
 * Single canonical hex encode/decode for the Worker runtime.
 *
 * Used by auth.ts (pubkey/df parsing), sanitize.ts (canonical_hash transport),
 * rate_limit.ts (key formatting), and DO/route code that mirrors hashes into
 * error envelopes. One implementation, no dependencies — per invariant #0 the
 * threat model does not require constant-time hex (we constant-time compare at
 * the string level in auth.ts).
 */

/** Encode a byte sequence as lowercase hex. Accepts `Uint8Array` or a plain
 *  `number[]` so call sites holding typed-array-like buffers don't need to
 *  materialize an extra copy. */
export function hexEncode(bytes: Uint8Array | number[]): string {
  let s = '';
  for (let i = 0; i < bytes.length; i++) {
    const b = bytes[i]!;
    s += (b & 0xff).toString(16).padStart(2, '0');
  }
  return s;
}

/** Decode a (case-insensitive) hex string. Throws on odd length or any
 *  non-hex character. */
export function hexDecode(hex: string): Uint8Array {
  if (hex.length % 2 !== 0) throw new Error('hex: odd length');
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    const hi = hexNibble(hex.charCodeAt(i * 2));
    const lo = hexNibble(hex.charCodeAt(i * 2 + 1));
    if (hi < 0 || lo < 0) throw new Error('hex: bad char');
    out[i] = (hi << 4) | lo;
  }
  return out;
}

function hexNibble(code: number): number {
  if (code >= 48 && code <= 57) return code - 48; // 0-9
  if (code >= 97 && code <= 102) return code - 87; // a-f
  if (code >= 65 && code <= 70) return code - 55; // A-F
  return -1;
}
