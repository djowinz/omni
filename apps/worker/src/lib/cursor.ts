/**
 * Opaque pagination cursor: base64url-encoded JSON `{ t, i }` where `t` is the
 * tie-breaker sort value (timestamp or string) and `i` is the row id.
 *
 * Non-ASCII JSON values (e.g. unicode tag names that might appear in admin
 * queries) round-trip cleanly because the base64url helpers go through
 * `TextEncoder` / `TextDecoder`.
 */
import { b64urlDecodeJson, b64urlEncodeJson } from './base64url';

export interface Cursor {
  t: number | string;
  i: string;
}

export function encodeCursor(c: Cursor): string {
  return b64urlEncodeJson(c);
}

export function decodeCursor(s: string): Cursor {
  const parsed = b64urlDecodeJson<Cursor>(s);
  if (
    parsed === null ||
    typeof parsed !== 'object' ||
    (typeof parsed.t !== 'number' && typeof parsed.t !== 'string') ||
    typeof parsed.i !== 'string'
  ) {
    throw new Error('decodeCursor: malformed cursor payload');
  }
  return parsed;
}
