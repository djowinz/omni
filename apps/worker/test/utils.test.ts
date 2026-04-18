import { describe, it, expect } from 'vitest';
import { isModerator } from '../src/lib/moderator';
import { encodeCursor, decodeCursor, type Cursor } from '../src/lib/cursor';
import { parseMultipart, MultipartError } from '../src/lib/multipart';
import type { Env } from '../src/env';

/**
 * Covers the three small utilities owned by W2T8:
 *  - moderator allowlist (case-insensitive, whitespace-tolerant)
 *  - opaque cursor round-trip
 *  - multipart/form-data parsing via native Request.formData()
 */

function mkEnv(pubkeys: string): Env {
  // Only `OMNI_ADMIN_PUBKEYS` is read by isModerator; cast to satisfy the
  // full Env shape without materializing R2/D1/KV/DO bindings in tests.
  return { OMNI_ADMIN_PUBKEYS: pubkeys } as unknown as Env;
}

describe('isModerator', () => {
  const K = 'aa'.repeat(32); // 64-hex dummy pubkey
  const K2 = 'bb'.repeat(32);

  it('returns true for an exact match', () => {
    expect(isModerator(K, mkEnv(K))).toBe(true);
  });

  it('matches case-insensitively', () => {
    expect(isModerator(K.toUpperCase(), mkEnv(K))).toBe(true);
    expect(isModerator(K, mkEnv(K.toUpperCase()))).toBe(true);
  });

  it('tolerates whitespace and empty entries', () => {
    const raw = `  ,${K.toUpperCase()}  ,, ${K2} ,`;
    expect(isModerator(K, mkEnv(raw))).toBe(true);
    expect(isModerator(K2, mkEnv(raw))).toBe(true);
  });

  it('returns false when not listed, empty, or undefined', () => {
    expect(isModerator(K, mkEnv(''))).toBe(false);
    expect(isModerator(K, mkEnv(K2))).toBe(false);
    expect(isModerator('', mkEnv(K))).toBe(false);
    // Env with OMNI_ADMIN_PUBKEYS undefined (e.g. pre-binding local tests).
    expect(isModerator(K, {} as unknown as Env)).toBe(false);
  });
});

describe('cursor encode/decode', () => {
  const shapes: Cursor[] = [
    { t: 0, i: '' },
    { t: 1712345678901, i: '01HX7QJ5Y0ABCD' },
    { t: '2026-04-14T00:00:00Z', i: 'id-with-dashes' },
    { t: 'tag:unicode-☃', i: 'row/42' },
    { t: Number.MAX_SAFE_INTEGER, i: 'x'.repeat(64) },
  ];

  for (const c of shapes) {
    it(`round-trips ${JSON.stringify(c)}`, () => {
      const s = encodeCursor(c);
      // base64url: no +, /, =.
      expect(s).not.toMatch(/[+/=]/);
      expect(decodeCursor(s)).toEqual(c);
    });
  }

  it('rejects malformed payload shape', () => {
    const bogus = encodeCursor({ t: 1, i: 'x' }).replace(/./, ''); // truncate
    expect(() => decodeCursor(bogus)).toThrow();
  });
});

describe('parseMultipart', () => {
  function mkReq(fd: FormData): Request {
    return new Request('https://example.invalid/upload', {
      method: 'POST',
      body: fd,
    });
  }

  it('returns Uint8Arrays for bundle + thumbnail when both present', async () => {
    const fd = new FormData();
    const bundleBytes = new Uint8Array([0x50, 0x4b, 0x03, 0x04]); // PK\x03\x04
    const thumbBytes = new Uint8Array([0x52, 0x49, 0x46, 0x46]); // RIFF
    fd.append('bundle', new Blob([bundleBytes], { type: 'application/zip' }), 'theme.omni');
    fd.append('thumbnail', new Blob([thumbBytes], { type: 'image/webp' }), 'thumb.webp');

    const parts = await parseMultipart(mkReq(fd));
    expect(parts.bundle).toBeInstanceOf(Uint8Array);
    expect(parts.thumbnail).toBeInstanceOf(Uint8Array);
    expect(Array.from(parts.bundle)).toEqual(Array.from(bundleBytes));
    expect(Array.from(parts.thumbnail)).toEqual(Array.from(thumbBytes));
  });

  it('throws MultipartError when bundle is missing', async () => {
    const fd = new FormData();
    fd.append('thumbnail', new Blob([new Uint8Array([1, 2, 3])], { type: 'image/webp' }), 't.webp');
    await expect(parseMultipart(mkReq(fd))).rejects.toBeInstanceOf(MultipartError);
  });

  it('throws MultipartError when thumbnail is missing', async () => {
    const fd = new FormData();
    fd.append(
      'bundle',
      new Blob([new Uint8Array([1, 2, 3])], { type: 'application/zip' }),
      't.omni',
    );
    await expect(parseMultipart(mkReq(fd))).rejects.toBeInstanceOf(MultipartError);
  });

  it('throws MultipartError when a part is a string instead of a file', async () => {
    const fd = new FormData();
    fd.append('bundle', 'not-a-file');
    fd.append('thumbnail', new Blob([new Uint8Array([1])], { type: 'image/webp' }), 't.webp');
    await expect(parseMultipart(mkReq(fd))).rejects.toBeInstanceOf(MultipartError);
  });
});
