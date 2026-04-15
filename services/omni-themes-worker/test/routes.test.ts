import { describe, it, expect } from "vitest";
import app from "../src/index";

/**
 * Tier C — in-process tests. No Miniflare boot, no DO, no real bindings.
 * These exercise Hono routing + the error envelope + the global
 * X-Omni-Version / X-Omni-Sanitize-Version middleware (plan #008 W4T14).
 *
 * For behavior that touches bindings or the DO, see test/do.test.ts (tier B)
 * and the per-route Miniflare tests.
 */

describe("global client-version middleware (W4T14)", () => {
  it("rejects authed-route requests missing X-Omni-Version with 400 Malformed", async () => {
    const res = await app.request("/v1/list", { method: "GET" });
    expect(res.status).toBe(400);
    const body = (await res.json()) as {
      error: { code: string; message: string };
      kind?: string;
      detail?: string;
    };
    expect(body.error.code).toBe("BAD_REQUEST");
    expect(body.kind).toBe("Malformed");
    expect(body.error.message).toMatch(/X-Omni-Version/);
  });

  it("rejects malformed (non-semver) X-Omni-Version with 400", async () => {
    const res = await app.request("/v1/list", {
      method: "GET",
      headers: { "X-Omni-Version": "not-a-semver", "X-Omni-Sanitize-Version": "1" },
    });
    expect(res.status).toBe(400);
    const body = (await res.json()) as { error: { code: string } };
    expect(body.error.code).toBe("BAD_REQUEST");
  });

  it("rejects requests missing X-Omni-Sanitize-Version with 400", async () => {
    const res = await app.request("/v1/list", {
      method: "GET",
      headers: { "X-Omni-Version": "0.1.0" },
    });
    expect(res.status).toBe(400);
    const body = (await res.json()) as {
      error: { message: string };
      kind?: string;
    };
    expect(body.kind).toBe("Malformed");
    expect(body.error.message).toMatch(/X-Omni-Sanitize-Version/);
  });

  it("passes through when both headers are valid (downstream auth may still reject)", async () => {
    const res = await app.request("/v1/list", {
      method: "GET",
      headers: { "X-Omni-Version": "0.1.0", "X-Omni-Sanitize-Version": "1" },
    });
    // We don't pin the exact downstream status (no JWS, no bindings); we only
    // assert the middleware did NOT short-circuit with the "missing header"
    // 400. Anything else (401, 500, etc.) means the gate let the request
    // through.
    if (res.status === 400) {
      const body = (await res.json()) as { error: { message: string } };
      expect(body.error.message).not.toMatch(/X-Omni-(Version|Sanitize-Version)/);
    }
  });
});

describe("config exemption — /v1/config/* is reachable without client-version headers", () => {
  it("GET /v1/config/vocab without headers is NOT blocked by middleware", async () => {
    const res = await app.request("/v1/config/vocab", { method: "GET" });
    // Middleware must not produce the X-Omni-Version 400. The route itself
    // may 500 without bindings in the Tier-C harness; that's fine — it proves
    // the gate was bypassed.
    if (res.status === 400) {
      const body = (await res.json()) as { error: { message: string } };
      expect(body.error.message).not.toMatch(/X-Omni-(Version|Sanitize-Version)/);
    }
  });

  it("GET /v1/config/limits without headers is NOT blocked by middleware", async () => {
    const res = await app.request("/v1/config/limits", { method: "GET" });
    if (res.status === 400) {
      const body = (await res.json()) as { error: { message: string } };
      expect(body.error.message).not.toMatch(/X-Omni-(Version|Sanitize-Version)/);
    }
  });
});

describe("download exemption — GET /v1/download/:id is reachable without headers", () => {
  it("GET /v1/download/<id> without headers is NOT blocked by middleware", async () => {
    const res = await app.request("/v1/download/some-id", { method: "GET" });
    // Again: the gate must not produce the missing-header 400. The route may
    // 404/500 without bindings; any response OTHER than a missing-header 400
    // proves the exemption works.
    if (res.status === 400) {
      const body = (await res.json()) as { error: { message: string } };
      expect(body.error.message).not.toMatch(/X-Omni-(Version|Sanitize-Version)/);
    }
  });
});

describe("admin route is mounted (W4T14)", () => {
  it("PATCH /v1/admin/vocab is reachable (not 404)", async () => {
    const res = await app.request("/v1/admin/vocab", {
      method: "PATCH",
      headers: {
        "X-Omni-Version": "0.1.0",
        "X-Omni-Sanitize-Version": "1",
        "content-type": "application/json",
      },
      body: "{}",
    });
    expect(res.status).not.toBe(404);
  });
});

describe("unknown routes", () => {
  it("unknown routes return 404 NOT_FOUND (with headers set)", async () => {
    const res = await app.request("/v1/does-not-exist", {
      method: "GET",
      headers: { "X-Omni-Version": "0.1.0", "X-Omni-Sanitize-Version": "1" },
    });
    expect(res.status).toBe(404);
    const body = (await res.json()) as { error: { code: string } };
    expect(body.error.code).toBe("NOT_FOUND");
  });
});
