/**
 * Cloudflare Worker binding types, mirroring `wrangler.toml`.
 * Any change here MUST be mirrored in `wrangler.toml` and vice versa.
 */
export interface Env {
  BLOBS: R2Bucket;
  META: D1Database;
  STATE: KVNamespace;
  BUNDLE_PROCESSOR: DurableObjectNamespace;
  OMNI_THEMES_ENV: "dev" | "prod";
  OMNI_THEMES_RATE_LIMIT_SCALE: string;
}
