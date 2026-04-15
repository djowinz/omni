import type { Env } from "../env";

/**
 * Returns true iff `pubkeyHex` is in the comma-separated `OMNI_ADMIN_PUBKEYS`
 * allowlist (case-insensitive, whitespace-tolerant, empty entries ignored).
 *
 * Single source of truth for moderator identity. Called by admin routes before
 * executing moderation actions; on false, caller must return Admin/NotModerator
 * per worker-api.md §3.
 */
export function isModerator(pubkeyHex: string, env: Env): boolean {
  const raw = env.OMNI_ADMIN_PUBKEYS ?? "";
  const normalized = pubkeyHex.trim().toLowerCase();
  if (!normalized) return false;
  return raw
    .split(",")
    .map((k) => k.trim().toLowerCase())
    .filter(Boolean)
    .includes(normalized);
}
