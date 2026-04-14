import { describe, it, expect } from "vitest";
import app from "../src/index";

/**
 * Tier C — in-process tests. No Miniflare boot, no DO, no real bindings.
 * These exercise Hono routing + the error envelope. For tests that touch
 * bindings or the DO, see test/do.test.ts (tier B).
 */

async function assertNotImplemented(res: Response, routeLabel: string) {
  expect(res.status, `expected 501 from ${routeLabel}`).toBe(501);
  expect(res.headers.get("content-type")).toContain("application/json");
  const body = (await res.json()) as { error: { code: string; message: string } };
  expect(body.error.code).toBe("NOT_IMPLEMENTED");
  expect(body.error.message.length).toBeGreaterThan(0);
}

describe("worker routes — every contract endpoint returns 501", () => {
  it("POST /v1/upload", async () => {
    const res = await app.request("/v1/upload", { method: "POST" });
    await assertNotImplemented(res, "POST /v1/upload");
  });

  it("GET /v1/download/:id", async () => {
    const res = await app.request("/v1/download/abc123", { method: "GET" });
    await assertNotImplemented(res, "GET /v1/download/:id");
  });

  it("GET /v1/list", async () => {
    const res = await app.request("/v1/list", { method: "GET" });
    await assertNotImplemented(res, "GET /v1/list");
  });

  it("GET /v1/artifact/:id", async () => {
    const res = await app.request("/v1/artifact/abc123", { method: "GET" });
    await assertNotImplemented(res, "GET /v1/artifact/:id");
  });

  it("PATCH /v1/artifact/:id", async () => {
    const res = await app.request("/v1/artifact/abc123", { method: "PATCH" });
    await assertNotImplemented(res, "PATCH /v1/artifact/:id");
  });

  it("DELETE /v1/artifact/:id", async () => {
    const res = await app.request("/v1/artifact/abc123", { method: "DELETE" });
    await assertNotImplemented(res, "DELETE /v1/artifact/:id");
  });

  it("POST /v1/report", async () => {
    const res = await app.request("/v1/report", { method: "POST" });
    await assertNotImplemented(res, "POST /v1/report");
  });

  it("GET /v1/config/vocab", async () => {
    const res = await app.request("/v1/config/vocab", { method: "GET" });
    await assertNotImplemented(res, "GET /v1/config/vocab");
  });

  it("GET /v1/config/limits", async () => {
    const res = await app.request("/v1/config/limits", { method: "GET" });
    await assertNotImplemented(res, "GET /v1/config/limits");
  });

  it("GET /v1/me/gallery", async () => {
    const res = await app.request("/v1/me/gallery", { method: "GET" });
    await assertNotImplemented(res, "GET /v1/me/gallery");
  });

  it("unknown routes return 404 NOT_FOUND (not 501)", async () => {
    const res = await app.request("/v1/does-not-exist", { method: "GET" });
    expect(res.status).toBe(404);
    const body = (await res.json()) as { error: { code: string } };
    expect(body.error.code).toBe("NOT_FOUND");
  });
});
