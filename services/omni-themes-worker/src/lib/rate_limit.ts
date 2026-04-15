/**
 * KV-backed rate limiter keyed on device fingerprint.
 *
 * See spec #008 §3 and invariant #10 (DF is the durable anchor; pubkey is
 * secondary). Read-then-write race is accepted per invariant #0 (low-volume
 * overlay utility, narrow threat model — no Durable Object counter).
 *
 * KV schema:
 *   - `quota:device:<df_hex>:<YYYY-MM-DD>`        integer daily counter
 *   - `quota:pubkey:<pubkey_hex>:<YYYY-MM-DD>`    integer daily counter
 *   - `quota:device:<df_hex>:<YYYY-MM-DDTHH:MM>`  integer per-minute counter (download)
 *   - `denylist:device:<df_hex>`                  presence flag → permanent deny
 *   - `denylist:pubkey:<pubkey_hex>`              presence flag → permanent deny
 *   - `df_pubkey_velocity:<df_hex>`               JSON `{p: string, t: number}[]`
 *                                                 distinct pubkeys in last 24h
 *   - `flags:vm:<df_hex>`                         presence flag → 25% limits
 */
import type { Env } from "../env";

export type RateLimitAction =
  | "upload_new"
  | "upload_update"
  | "upload_new_bundle"
  | "download"
  | "report";

export interface RateLimitResult {
  allowed: boolean;
  retry_after?: number;
  /**
   * When true, the caller must satisfy a Turnstile challenge before the
   * request proceeds. Mapped by the route handler to `TURNSTILE_REQUIRED`.
   * Triggered when more than 3 distinct pubkeys are observed from a single
   * device fingerprint in a 24h window.
   */
  turnstile?: boolean;
}

interface Window {
  /** "day" = YYYY-MM-DD, "minute" = YYYY-MM-DDTHH:MM */
  kind: "day" | "minute";
  /** Suffix used in the KV key. */
  suffix: string;
  /** Seconds until the window rolls over (upper bound; used for retry_after). */
  secondsUntilRollover: number;
  /** KV TTL seconds (>= 60 required by Cloudflare). */
  ttl: number;
}

interface Limits {
  perDeviceDay?: number;
  perPubkeyDay?: number;
  perDeviceMinute?: number;
}

const PROD_LIMITS: Record<RateLimitAction, Limits> = {
  upload_new: { perDeviceDay: 5, perPubkeyDay: 5 },
  upload_update: { perDeviceDay: 30, perPubkeyDay: 30 },
  upload_new_bundle: { perDeviceDay: 3, perPubkeyDay: 3 },
  download: { perDeviceMinute: 60 },
  report: { perDeviceDay: 20 },
};

const VELOCITY_LIMIT_DISTINCT_PUBKEYS = 3;
const VELOCITY_WINDOW_MS = 24 * 60 * 60 * 1000;

/** Per-minute rate-limit window. */
const MINUTE_WINDOW_SECONDS = 60;
/**
 * TTL slop on top of the raw window. Invariant #0 accepts the read-then-write
 * race; we keep counters around past rollover so a late writer can't roll the
 * bucket over and recharge the quota mid-window.
 */
const MINUTE_TTL_SLOP_SECONDS = 60;
const DAY_TTL_SLOP_SECONDS = 60;
/** Cloudflare KV requires `expirationTtl >= 60`. */
const KV_MIN_TTL_SECONDS = 60;

function pad2(n: number): string {
  return n < 10 ? `0${n}` : String(n);
}

function dayWindow(now: Date): Window {
  const yyyy = now.getUTCFullYear();
  const mm = pad2(now.getUTCMonth() + 1);
  const dd = pad2(now.getUTCDate());
  const suffix = `${yyyy}-${mm}-${dd}`;
  const next = Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate() + 1);
  const secondsUntilRollover = Math.max(1, Math.ceil((next - now.getTime()) / 1000));
  return {
    kind: "day",
    suffix,
    secondsUntilRollover,
    ttl: Math.max(KV_MIN_TTL_SECONDS, secondsUntilRollover + DAY_TTL_SLOP_SECONDS),
  };
}

function minuteWindow(now: Date): Window {
  const yyyy = now.getUTCFullYear();
  const mm = pad2(now.getUTCMonth() + 1);
  const dd = pad2(now.getUTCDate());
  const hh = pad2(now.getUTCHours());
  const mi = pad2(now.getUTCMinutes());
  const suffix = `${yyyy}-${mm}-${dd}T${hh}:${mi}`;
  const next = Date.UTC(
    now.getUTCFullYear(),
    now.getUTCMonth(),
    now.getUTCDate(),
    now.getUTCHours(),
    now.getUTCMinutes() + 1,
  );
  const secondsUntilRollover = Math.max(1, Math.ceil((next - now.getTime()) / 1000));
  return {
    kind: "minute",
    suffix,
    secondsUntilRollover,
    ttl: MINUTE_WINDOW_SECONDS + MINUTE_TTL_SLOP_SECONDS,
  };
}

function scale(env: Env): number {
  const raw = env.OMNI_THEMES_RATE_LIMIT_SCALE ?? "1";
  const parsed = Number(raw);
  if (!Number.isFinite(parsed) || parsed <= 0) return 1;
  return parsed;
}

async function readCounter(kv: KVNamespace, key: string): Promise<number> {
  const raw = await kv.get(key);
  if (raw === null) return 0;
  const n = Number(raw);
  return Number.isFinite(n) && n >= 0 ? Math.floor(n) : 0;
}

async function bumpCounter(
  kv: KVNamespace,
  key: string,
  ttl: number,
  current: number,
): Promise<void> {
  // Read-then-write race is accepted (invariant #0).
  await kv.put(key, String(current + 1), { expirationTtl: ttl });
}

interface VelocityEntry {
  p: string;
  t: number;
}

async function checkAndUpdateVelocity(
  kv: KVNamespace,
  df_hex: string,
  pubkey_hex: string,
  nowMs: number,
): Promise<boolean> {
  const key = `df_pubkey_velocity:${df_hex}`;
  const raw = await kv.get(key);
  let entries: VelocityEntry[] = [];
  if (raw !== null) {
    try {
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed)) {
        entries = parsed.filter(
          (e): e is VelocityEntry =>
            e !== null &&
            typeof e === "object" &&
            typeof (e as VelocityEntry).p === "string" &&
            typeof (e as VelocityEntry).t === "number",
        );
      }
    } catch {
      entries = [];
    }
  }

  const cutoff = nowMs - VELOCITY_WINDOW_MS;
  entries = entries.filter((e) => e.t >= cutoff);

  const existing = entries.find((e) => e.p === pubkey_hex);
  if (existing) {
    existing.t = nowMs;
  } else {
    entries.push({ p: pubkey_hex, t: nowMs });
  }

  const distinctCount = new Set(entries.map((e) => e.p)).size;

  await kv.put(key, JSON.stringify(entries), {
    expirationTtl: Math.ceil(VELOCITY_WINDOW_MS / 1000) + 60,
  });

  return distinctCount > VELOCITY_LIMIT_DISTINCT_PUBKEYS;
}

function applyScaleAndVm(limit: number, scaleFactor: number, vmFlag: boolean): number {
  const scaled = limit * scaleFactor;
  if (vmFlag) {
    // 25% of nominal — floor so VM devices never exceed the documented cap.
    return Math.max(1, Math.floor(scaled * 0.25));
  }
  return Math.max(1, Math.floor(scaled));
}

/**
 * Check the limits for the given action and, if allowed, increment the
 * relevant counters. Returns `{allowed: false, retry_after}` when the window
 * is exhausted; `{allowed: false, turnstile: true}` when the device has
 * cycled more than 3 distinct pubkeys in 24h; `{allowed: false}` with no
 * retry hint when the device or pubkey is on a denylist.
 */
export async function checkAndIncrement(
  env: Env,
  df_hex: string,
  pubkey_hex: string,
  action: RateLimitAction,
): Promise<RateLimitResult> {
  const kv = env.STATE;
  const now = new Date();

  // --- Denylist: permanent deny until revoked. ---
  const [denyDev, denyPub] = await Promise.all([
    kv.get(`denylist:device:${df_hex}`),
    pubkey_hex ? kv.get(`denylist:pubkey:${pubkey_hex}`) : Promise.resolve(null),
  ]);
  if (denyDev !== null || denyPub !== null) {
    return { allowed: false };
  }

  // --- VM flag → 25% of nominal. ---
  const vmFlag = (await kv.get(`flags:vm:${df_hex}`)) !== null;

  // --- Turnstile velocity check (only meaningful for authed actions). ---
  if (pubkey_hex && action !== "download") {
    const tripped = await checkAndUpdateVelocity(kv, df_hex, pubkey_hex, now.getTime());
    if (tripped) {
      return { allowed: false, turnstile: true };
    }
  }

  const nominal = PROD_LIMITS[action];
  const factor = scale(env);

  const checks: Array<{ key: string; limit: number; window: Window }> = [];

  if (nominal.perDeviceDay !== undefined) {
    const w = dayWindow(now);
    checks.push({
      key: `quota:device:${df_hex}:${w.suffix}`,
      limit: applyScaleAndVm(nominal.perDeviceDay, factor, vmFlag),
      window: w,
    });
  }
  if (nominal.perPubkeyDay !== undefined && pubkey_hex) {
    const w = dayWindow(now);
    checks.push({
      key: `quota:pubkey:${pubkey_hex}:${w.suffix}`,
      limit: applyScaleAndVm(nominal.perPubkeyDay, factor, vmFlag),
      window: w,
    });
  }
  if (nominal.perDeviceMinute !== undefined) {
    const w = minuteWindow(now);
    checks.push({
      key: `quota:device:${df_hex}:${w.suffix}`,
      limit: applyScaleAndVm(nominal.perDeviceMinute, factor, vmFlag),
      window: w,
    });
  }

  // Read all counters in parallel; pick the tightest-blocked window for retry_after.
  const currents = await Promise.all(checks.map((c) => readCounter(kv, c.key)));
  for (let i = 0; i < checks.length; i++) {
    if (currents[i] >= checks[i].limit) {
      return { allowed: false, retry_after: checks[i].window.secondsUntilRollover };
    }
  }

  // All counters under their caps → increment every relevant key.
  await Promise.all(
    checks.map((c, i) => bumpCounter(kv, c.key, c.window.ttl, currents[i])),
  );

  return { allowed: true };
}
