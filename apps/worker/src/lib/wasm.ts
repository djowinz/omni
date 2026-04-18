/**
 * Lazy singleton loader for the three wasm-bindgen modules shipped at
 * `src/wasm/*.js` + `*.wasm`. Called by `canonical.ts`, `sanitize.ts`, and
 * the `BundleProcessor` DO. Instantiation is deferred to first `loadWasm()`
 * invocation so cold-start cost isn't paid on every isolate boot.
 *
 * Cloudflare Workers: `*.wasm` imports resolve to `WebAssembly.Module`
 * instances (per the `[[rules]] type = "CompiledWasm"` binding in
 * `wrangler.toml`), so we feed the module to the sync `initSync` export
 * of each wasm-bindgen JS shim. The default async `init(...)` would try to
 * `fetch()` a relative URL at runtime, which the Workers runtime forbids.
 */
import * as BundleNS from '../wasm/omni_bundle.js';
import * as IdentityNS from '../wasm/omni_identity.js';
import * as SanitizeNS from '../wasm/omni_sanitize.js';

import bundleModule from '../wasm/omni_bundle.wasm';
import identityModule from '../wasm/omni_identity.wasm';
import sanitizeModule from '../wasm/omni_sanitize.wasm';

export interface WasmBindings {
  bundle: typeof BundleNS;
  identity: typeof IdentityNS;
  sanitize: typeof SanitizeNS;
}

let cached: WasmBindings | null = null;

/**
 * Instantiate (once) and return all three wasm-bindgen namespaces. Safe to
 * call concurrently from multiple requests — `initSync` is idempotent when
 * the module is already bound.
 */
export async function loadWasm(): Promise<WasmBindings> {
  if (cached) return cached;
  // initSync is synchronous; Promise.all kept for symmetry with the async
  // wasm-bindgen default export pattern and to future-proof if any module
  // ever needs async setup.
  await Promise.all([
    Promise.resolve(BundleNS.initSync({ module: bundleModule })),
    Promise.resolve(IdentityNS.initSync({ module: identityModule })),
    Promise.resolve(SanitizeNS.initSync({ module: sanitizeModule })),
  ]);
  cached = { bundle: BundleNS, identity: IdentityNS, sanitize: SanitizeNS };
  return cached;
}

/** Test-only: reset the cache so a fresh load can be asserted. */
export function __resetWasmForTests(): void {
  cached = null;
}
