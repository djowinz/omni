/**
 * RFC 4648 §5 base64url (URL-safe, no padding) helpers for the Workers
 * runtime.
 *
 * Uses `atob` / `btoa` + `TextEncoder` / `TextDecoder` only — no `Buffer`
 * (not available in workerd). Consumers: `auth.ts` (JWS segment decode),
 * `cursor.ts` (opaque pagination cursors), `sanitize.ts` (DO byte payload
 * transport).
 */

/** Encode bytes as base64url with no trailing `=` padding. */
export function b64urlEncode(bytes: Uint8Array): string {
  let bin = '';
  for (let i = 0; i < bytes.length; i++) {
    bin += String.fromCharCode(bytes[i]!);
  }
  return btoa(bin).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
}

/** Decode a base64url string. Tolerates missing `=` padding. */
export function b64urlDecode(s: string): Uint8Array {
  const rem = s.length % 4;
  const pad = rem === 0 ? '' : '='.repeat(4 - rem);
  const b64 = s.replace(/-/g, '+').replace(/_/g, '/') + pad;
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

/** `JSON.stringify` then UTF-8 → base64url. */
export function b64urlEncodeJson(obj: unknown): string {
  const bytes = new TextEncoder().encode(JSON.stringify(obj));
  return b64urlEncode(bytes);
}

/** base64url → UTF-8 → `JSON.parse`. Caller asserts the `T` shape. */
export function b64urlDecodeJson<T = unknown>(s: string): T {
  const bytes = b64urlDecode(s);
  const json = new TextDecoder().decode(bytes);
  return JSON.parse(json) as T;
}
