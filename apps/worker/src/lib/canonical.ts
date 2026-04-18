/**
 * Thin wrapper around the `omni-bundle` WASM `canonicalHash` export. The
 * canonical hash = SHA-256(RFC 8785 JCS manifest bytes) per architectural
 * invariant #6. Callers pass a parsed manifest object; we return the 32-byte
 * digest as a `Uint8Array`.
 */
import { loadWasm } from './wasm';

export async function canonicalHash(manifest: object): Promise<Uint8Array> {
  const { bundle } = await loadWasm();
  return bundle.canonicalHash(manifest);
}
