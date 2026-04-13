/**
 * Sanitize pipeline loader — STUB.
 * Retro decision: ALL uploads (theme + bundle) route through the
 * BundleProcessor Durable Object in #008. This module is reserved for
 * the WASM loader that will live inside that DO.
 */

export class SanitizeNotImplementedError extends Error {
  constructor() {
    super("sanitize.load is a stub — implemented in sub-spec #008 inside BundleProcessor");
  }
}

export async function load(): Promise<never> {
  throw new SanitizeNotImplementedError();
}
