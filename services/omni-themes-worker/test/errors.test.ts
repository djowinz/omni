import { describe, it, expect } from "vitest";
import { errorResponse, notImplemented } from "../src/lib/errors";
import type { ContentfulStatusCode } from "hono/utils/http-status";

/**
 * These tests exercise the helpers directly (no Hono context needed).
 * The helpers are also designed to be called with a Hono `Context` — that
 * path is covered transitively by test/routes.test.ts.
 */

describe("errorResponse", () => {
  it("produces a Response with the given status, code, and message", async () => {
    const res = errorResponse(400 as ContentfulStatusCode, "BAD_REQUEST", "bad input");
    expect(res.status).toBe(400);
    expect(res.headers.get("content-type")).toContain("application/json");
    const body = await res.json();
    expect(body).toEqual({
      error: { code: "BAD_REQUEST", message: "bad input" },
    });
  });

  it("includes retry_after when provided", async () => {
    const res = errorResponse(429 as ContentfulStatusCode, "RATE_LIMITED", "slow down", 60);
    const body = (await res.json()) as { error: { retry_after?: number } };
    expect(body.error.retry_after).toBe(60);
  });
});

describe("notImplemented", () => {
  it("returns a 501 with code NOT_IMPLEMENTED and the route name in the message", async () => {
    const res = notImplemented("/v1/upload");
    expect(res.status).toBe(501);
    const body = (await res.json()) as { error: { code: string; message: string } };
    expect(body.error.code).toBe("NOT_IMPLEMENTED");
    expect(body.error.message).toContain("/v1/upload");
  });
});
