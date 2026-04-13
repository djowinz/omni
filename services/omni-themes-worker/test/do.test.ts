import { describe, it, expect } from "vitest";
import { env } from "cloudflare:test";
import type { Env } from "../src/env";

/**
 * Tier B — runs inside Miniflare. Proves the BundleProcessor DO binding
 * is registered, reachable, and returns the 501 envelope. This is the
 * anchor test #008 extends when real sanitize logic lands.
 */

// @cloudflare/vitest-pool-workers exposes `env` typed as ProvidedEnv.
// Augment its type to match our real Env so TypeScript knows about BUNDLE_PROCESSOR.
declare module "cloudflare:test" {
  interface ProvidedEnv extends Env {}
}

describe("BundleProcessor DO binding", () => {
  it("is reachable and returns 501 with NOT_IMPLEMENTED", async () => {
    const id = env.BUNDLE_PROCESSOR.idFromName("smoke");
    const stub = env.BUNDLE_PROCESSOR.get(id);
    const res = await stub.fetch("https://do.internal/sanitize", { method: "POST" });
    expect(res.status).toBe(501);
    const body = (await res.json()) as { error: { code: string } };
    expect(body.error.code).toBe("NOT_IMPLEMENTED");
  });
});
