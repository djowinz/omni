import { describe, it, expect, beforeEach } from "vitest";
import { env } from "cloudflare:test";
import type { Env } from "../src/env";
import { checkAndIncrement, type RateLimitAction } from "../src/lib/rate_limit";

/**
 * Tier B — runs inside Miniflare. Exercises the DF-keyed KV rate limiter
 * against a real KV binding. Covers plan #008 W2T6 step 1:
 *   - every action reaches its cap and the next call is denied
 *   - OMNI_THEMES_RATE_LIMIT_SCALE=10 multiplies limits; "1" preserves prod
 *   - VM flag reduces to 25% of nominal
 *   - distinct pubkey velocity > 3 trips the turnstile signal
 *   - denylist flag immediately denies without retry_after
 *
 * Per plan note: do NOT assert exact counts at high concurrency; KV
 * read-then-write race is accepted (invariant #0). Tests exercise serial
 * sequences so counter values are deterministic.
 */

declare module "cloudflare:test" {
  interface ProvidedEnv extends Env {}
}

// Each test uses a fresh DF so KV state from prior tests doesn't interfere.
let counter = 0;
function freshDf(label: string): string {
  counter += 1;
  return `df_${label}_${counter.toString(16).padStart(8, "0")}`;
}
function freshPub(label: string): string {
  counter += 1;
  return `pub_${label}_${counter.toString(16).padStart(8, "0")}`;
}

/** Build an Env view whose OMNI_THEMES_RATE_LIMIT_SCALE we can override per-test. */
function withScale(scale: string): Env {
  return { ...env, OMNI_THEMES_RATE_LIMIT_SCALE: scale };
}

async function clearVelocity(df: string): Promise<void> {
  await env.STATE.delete(`df_pubkey_velocity:${df}`);
}

beforeEach(async () => {
  // Nothing global to reset — each test keys on a fresh DF.
});

describe("checkAndIncrement — action caps (scale=1)", () => {
  it.each<[RateLimitAction, number]>([
    ["upload_new", 5],
    ["upload_update", 30],
    ["upload_new_bundle", 3],
    ["report", 20],
  ])("%s allows %d calls/day then denies with retry_after", async (action, cap) => {
    const df = freshDf(action);
    const pub = freshPub(action);
    const e = withScale("1");

    // Use a single stable pubkey so the velocity list stays at 1 distinct entry.
    for (let i = 0; i < cap; i++) {
      const r = await checkAndIncrement(e, df, pub, action);
      expect(r.allowed, `call ${i + 1}/${cap} should be allowed`).toBe(true);
    }

    const denied = await checkAndIncrement(e, df, pub, action);
    expect(denied.allowed).toBe(false);
    expect(denied.turnstile).toBeUndefined();
    expect(denied.retry_after).toBeGreaterThan(0);
    // Daily window: retry_after should be <= 24h.
    expect(denied.retry_after!).toBeLessThanOrEqual(24 * 60 * 60);
  });

  it("download allows 60 calls/minute/device then denies with short retry_after", async () => {
    const df = freshDf("download");
    const e = withScale("1");

    for (let i = 0; i < 60; i++) {
      const r = await checkAndIncrement(e, df, "", "download");
      expect(r.allowed).toBe(true);
    }
    const denied = await checkAndIncrement(e, df, "", "download");
    expect(denied.allowed).toBe(false);
    // Per-minute window → retry in seconds, at most 60.
    expect(denied.retry_after).toBeGreaterThan(0);
    expect(denied.retry_after!).toBeLessThanOrEqual(60);
  });
});

describe("checkAndIncrement — OMNI_THEMES_RATE_LIMIT_SCALE", () => {
  it("scale=10 multiplies cap (upload_new → 50)", async () => {
    const df = freshDf("scale10");
    const pub = freshPub("scale10");
    const e = withScale("10");

    for (let i = 0; i < 50; i++) {
      const r = await checkAndIncrement(e, df, pub, "upload_new");
      expect(r.allowed, `call ${i + 1}/50`).toBe(true);
    }
    const denied = await checkAndIncrement(e, df, pub, "upload_new");
    expect(denied.allowed).toBe(false);
  });

  it("scale=1 preserves production cap (upload_new_bundle → 3)", async () => {
    const df = freshDf("scale1");
    const pub = freshPub("scale1");
    const e = withScale("1");

    for (let i = 0; i < 3; i++) {
      const r = await checkAndIncrement(e, df, pub, "upload_new_bundle");
      expect(r.allowed).toBe(true);
    }
    const denied = await checkAndIncrement(e, df, pub, "upload_new_bundle");
    expect(denied.allowed).toBe(false);
  });
});

describe("checkAndIncrement — VM flag", () => {
  it("flags:vm:<df> reduces nominal to 25% (upload_update → 7)", async () => {
    const df = freshDf("vm");
    const pub = freshPub("vm");
    await env.STATE.put(`flags:vm:${df}`, "1");
    const e = withScale("1");

    // 30 * 0.25 = 7 (floor), so 7 allowed then deny.
    for (let i = 0; i < 7; i++) {
      const r = await checkAndIncrement(e, df, pub, "upload_update");
      expect(r.allowed, `call ${i + 1}/7`).toBe(true);
    }
    const denied = await checkAndIncrement(e, df, pub, "upload_update");
    expect(denied.allowed).toBe(false);
  });

  it("VM flag also reduces download minute cap (60 → 15)", async () => {
    const df = freshDf("vmdl");
    await env.STATE.put(`flags:vm:${df}`, "1");
    const e = withScale("1");

    for (let i = 0; i < 15; i++) {
      const r = await checkAndIncrement(e, df, "", "download");
      expect(r.allowed).toBe(true);
    }
    const denied = await checkAndIncrement(e, df, "", "download");
    expect(denied.allowed).toBe(false);
  });
});

describe("checkAndIncrement — denylist", () => {
  it("denylist:device immediately denies without retry_after", async () => {
    const df = freshDf("denyDev");
    const pub = freshPub("denyDev");
    await env.STATE.put(`denylist:device:${df}`, "1");
    const e = withScale("1");

    const r = await checkAndIncrement(e, df, pub, "upload_new");
    expect(r.allowed).toBe(false);
    expect(r.retry_after).toBeUndefined();
    expect(r.turnstile).toBeUndefined();
  });

  it("denylist:pubkey immediately denies without retry_after", async () => {
    const df = freshDf("denyPub");
    const pub = freshPub("denyPub");
    await env.STATE.put(`denylist:pubkey:${pub}`, "1");
    const e = withScale("1");

    const r = await checkAndIncrement(e, df, pub, "upload_new");
    expect(r.allowed).toBe(false);
    expect(r.retry_after).toBeUndefined();
  });

  it("denylist takes precedence over a healthy quota", async () => {
    const df = freshDf("denyPrecedence");
    const pub = freshPub("denyPrecedence");
    await env.STATE.put(`denylist:device:${df}`, "1");
    const e = withScale("10"); // plenty of quota, still denied

    const r = await checkAndIncrement(e, df, pub, "report");
    expect(r.allowed).toBe(false);
  });
});

describe("checkAndIncrement — pubkey velocity turnstile", () => {
  it("4th distinct pubkey from the same DF trips the turnstile signal", async () => {
    const df = freshDf("velocity");
    await clearVelocity(df);
    const e = withScale("10"); // avoid hitting quota caps during this test

    // 3 distinct pubkeys: allowed (boundary is > 3).
    for (let i = 0; i < 3; i++) {
      const r = await checkAndIncrement(e, df, `pubkey_${df}_${i}`, "upload_new");
      expect(r.allowed, `pubkey ${i}`).toBe(true);
      expect(r.turnstile).toBeUndefined();
    }

    // 4th distinct pubkey → distinctCount becomes 4 > 3 → turnstile.
    const tripped = await checkAndIncrement(e, df, `pubkey_${df}_new`, "upload_new");
    expect(tripped.allowed).toBe(false);
    expect(tripped.turnstile).toBe(true);
    expect(tripped.retry_after).toBeUndefined();
  });

  it("repeating the same pubkey does not grow distinct count", async () => {
    const df = freshDf("velocitySame");
    await clearVelocity(df);
    const pub = freshPub("velocitySame");
    const e = withScale("10");

    for (let i = 0; i < 5; i++) {
      const r = await checkAndIncrement(e, df, pub, "upload_new");
      expect(r.allowed, `call ${i}`).toBe(true);
      expect(r.turnstile).toBeUndefined();
    }
  });

  it("download action does not consume velocity budget", async () => {
    const df = freshDf("velocityDl");
    await clearVelocity(df);
    const e = withScale("10");

    // 4 distinct pubkeys via download — velocity must remain empty.
    for (let i = 0; i < 4; i++) {
      const r = await checkAndIncrement(e, df, `pubkey_${df}_${i}`, "download");
      expect(r.allowed).toBe(true);
      expect(r.turnstile).toBeUndefined();
    }
    // Now use a 5th distinct pubkey on an authed action — should still be allowed
    // (velocity list is empty because download didn't record).
    const r = await checkAndIncrement(e, df, `pubkey_${df}_authed`, "upload_new");
    expect(r.allowed).toBe(true);
  });
});

describe("checkAndIncrement — counter monotonicity", () => {
  it("daily device counter increases strictly under serial calls", async () => {
    const df = freshDf("mono");
    const pub = freshPub("mono");
    const e = withScale("10");

    const now = new Date();
    const y = now.getUTCFullYear();
    const m = String(now.getUTCMonth() + 1).padStart(2, "0");
    const d = String(now.getUTCDate()).padStart(2, "0");
    const key = `quota:device:${df}:${y}-${m}-${d}`;

    let prev = 0;
    for (let i = 0; i < 4; i++) {
      const r = await checkAndIncrement(e, df, pub, "report");
      expect(r.allowed).toBe(true);
      const raw = await env.STATE.get(key);
      const cur = raw === null ? 0 : Number(raw);
      expect(cur).toBeGreaterThan(prev);
      prev = cur;
    }
  });
});
