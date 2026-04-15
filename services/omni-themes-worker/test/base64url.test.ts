import { describe, it, expect } from "vitest";
import {
  b64urlDecode,
  b64urlEncode,
  b64urlDecodeJson,
  b64urlEncodeJson,
} from "../src/lib/base64url";

describe("b64url encode/decode", () => {
  it("is URL-safe (no +, /, =)", () => {
    const bytes = new Uint8Array([0xfb, 0xff, 0xfe, 0x00, 0x01]);
    const s = b64urlEncode(bytes);
    expect(s).not.toMatch(/[+/=]/);
  });

  it("round-trips random bytes of varying length", () => {
    for (const len of [0, 1, 2, 3, 4, 17, 64, 257]) {
      const bytes = new Uint8Array(len);
      for (let i = 0; i < len; i++) bytes[i] = (i * 17 + 3) & 0xff;
      expect(Array.from(b64urlDecode(b64urlEncode(bytes)))).toEqual(Array.from(bytes));
    }
  });

  it("tolerates missing padding on decode", () => {
    const s = b64urlEncode(new Uint8Array([1, 2, 3, 4, 5])); // len 5 → needs padding internally
    expect(s).not.toMatch(/=/);
    expect(Array.from(b64urlDecode(s))).toEqual([1, 2, 3, 4, 5]);
  });
});

describe("b64url JSON helpers", () => {
  it("round-trips typical objects", () => {
    const o = { a: 1, b: "hello", c: [true, false, null] };
    expect(b64urlDecodeJson(b64urlEncodeJson(o))).toEqual(o);
  });

  it("handles non-ASCII values", () => {
    const o = { t: "tag:unicode-☃", i: "row/42" };
    expect(b64urlDecodeJson(b64urlEncodeJson(o))).toEqual(o);
  });
});
