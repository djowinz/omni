/**
 * Opaque pagination cursor: base64url-encoded JSON `{ t, i }` where `t` is the
 * tie-breaker sort value (timestamp or string) and `i` is the row id.
 *
 * Workers runtime has `atob`/`btoa` but no `Buffer`; we also go through
 * `TextEncoder`/`TextDecoder` so non-ASCII JSON values (e.g. unicode tag names
 * that might appear in admin queries) round-trip cleanly.
 */

export interface Cursor {
  t: number | string;
  i: string;
}

function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return btoa(bin);
}

function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function encodeCursor(c: Cursor): string {
  const bytes = new TextEncoder().encode(JSON.stringify(c));
  return bytesToBase64(bytes)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

export function decodeCursor(s: string): Cursor {
  const pad = s.length % 4 === 0 ? "" : "=".repeat(4 - (s.length % 4));
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/") + pad;
  const bytes = base64ToBytes(b64);
  const json = new TextDecoder().decode(bytes);
  const parsed = JSON.parse(json) as Cursor;
  if (
    parsed === null ||
    typeof parsed !== "object" ||
    (typeof parsed.t !== "number" && typeof parsed.t !== "string") ||
    typeof parsed.i !== "string"
  ) {
    throw new Error("decodeCursor: malformed cursor payload");
  }
  return parsed;
}
