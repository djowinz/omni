/**
 * Cloudflare Worker binding types, mirroring `wrangler.toml`.
 * Any change here MUST be mirrored in `wrangler.toml` and vice versa.
 */
export interface Env {
  BLOBS: R2Bucket;
  META: D1Database;
  STATE: KVNamespace;
  BUNDLE_PROCESSOR: DurableObjectNamespace;
  OMNI_THEMES_ENV: 'dev' | 'prod';
  OMNI_THEMES_RATE_LIMIT_SCALE: string;
  /**
   * Comma-separated lowercase hex Ed25519 pubkeys allowlisted as moderators.
   * Empty string = no moderators (admin routes return 403). See
   * `src/lib/moderator.ts`.
   */
  OMNI_ADMIN_PUBKEYS: string;
}
