import type { Env } from "../env";

/**
 * Single sanitize entry point for every upload (theme + bundle), per retro
 * decision locked in sub-spec §Retro findings #2.
 *
 * Sub-spec #008 will:
 *   - parse Content-Type / manifest kind off the incoming Request
 *   - dispatch to sanitize_theme() (CSS) or sanitize_bundle() (ZIP)
 *   - write sanitized bytes to R2, record metadata in D1
 *   - return the contract-shaped upload response or a structured error
 *
 * This sub-spec (#007) exports the class so the DO binding is registered
 * and wrangler's `new_classes = ["BundleProcessor"]` migration resolves.
 */
export class BundleProcessor {
  constructor(
    private readonly state: DurableObjectState,
    private readonly env: Env,
  ) {}

  async fetch(_req: Request): Promise<Response> {
    // `state` and `env` are held for #008. Reference them here so
    // --noUnusedLocals / --noUnusedParameters stay happy without the
    // `_`-prefix rename, which would obscure the field names #008 expects.
    void this.state;
    void this.env;
    return new Response(
      JSON.stringify({
        error: {
          code: "NOT_IMPLEMENTED",
          message:
            "BundleProcessor sanitize pipeline is not yet implemented (sub-spec #008)",
        },
      }),
      { status: 501, headers: { "content-type": "application/json; charset=utf-8" } },
    );
  }
}
