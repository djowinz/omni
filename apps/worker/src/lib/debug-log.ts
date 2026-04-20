/**
 * makeDebugLog — factory for an env-gated breadcrumb logger.
 *
 * Returns a `(...args) => void` closure. When `env.OMNI_DEBUG === '1'`
 * the closure delegates to `console.log`; otherwise it no-ops.
 *
 * The factory shape (as opposed to a module-level flag) matches how
 * Cloudflare Workers expose per-request bindings: `env` is available
 * only inside the fetch handler, not at module-import time. Callers
 * make one logger per request (or per module-scope init that
 * captures `env` from the handler).
 *
 * Only the literal string "1" enables logging. Empty string, unset,
 * and any other value (including "true") all no-op. This matches the
 * Worker convention of treating env vars as opt-in flags where the
 * explicit "1" is the unambiguous on-signal.
 *
 * See OWI-5.
 */

const NOOP: (...args: unknown[]) => void = () => {};

export function makeDebugLog(env: { OMNI_DEBUG?: string }): (...args: unknown[]) => void {
  if (env.OMNI_DEBUG === '1') {
    return (...args) => console.log(...args);
  }
  return NOOP;
}
