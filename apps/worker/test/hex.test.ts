import { describe, it, expect } from 'vitest';
import { hexEncode, hexDecode } from '../src/lib/hex';

describe('hex encode/decode', () => {
  it('round-trips random bytes', () => {
    const bytes = new Uint8Array(64);
    for (let i = 0; i < bytes.length; i++) bytes[i] = (i * 37) & 0xff;
    expect(hexDecode(hexEncode(bytes))).toEqual(bytes);
  });

  it('produces lowercase output', () => {
    expect(hexEncode(new Uint8Array([0x0a, 0xff, 0xab]))).toBe('0affab');
  });

  it('accepts a plain number[] as input', () => {
    expect(hexEncode([0, 15, 255])).toBe('000fff');
  });

  it('decodes upper + lower case', () => {
    expect(Array.from(hexDecode('DeAdBeEf'))).toEqual([0xde, 0xad, 0xbe, 0xef]);
  });

  it('throws on odd length', () => {
    expect(() => hexDecode('abc')).toThrow(/odd length/);
  });

  it('throws on non-hex chars', () => {
    expect(() => hexDecode('zz')).toThrow(/bad char/);
    expect(() => hexDecode('0g')).toThrow(/bad char/);
  });

  it('handles empty input', () => {
    expect(hexEncode(new Uint8Array([]))).toBe('');
    expect(hexDecode('')).toEqual(new Uint8Array([]));
  });
});
