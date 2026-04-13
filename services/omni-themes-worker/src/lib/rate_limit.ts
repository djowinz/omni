/**
 * KV-backed rate limiter — STUB.
 * Full implementation lands in sub-spec #008. Quotas defined in umbrella §6.3.
 */
import type { Env } from "../env";

export type Quota =
  | "upload_new"
  | "upload_update"
  | "download"
  | "report_submit";

export class RateLimitNotImplementedError extends Error {
  constructor() {
    super("rate_limit.check is a stub — implemented in sub-spec #008");
  }
}

export async function check(
  _env: Env,
  _quota: Quota,
  _deviceFingerprintHex: string,
  _pubkeyHex?: string,
): Promise<{ allowed: boolean; retryAfter?: number }> {
  throw new RateLimitNotImplementedError();
}
