/**
 * Request signature verification — STUB.
 * Full implementation lands in sub-spec #008 against
 * docs/superpowers/specs/contracts/worker-api.md §2 (canonical signing string).
 */
import type { Env } from "../env";

export interface VerifiedRequest {
  pubkey: Uint8Array;
  deviceFingerprint: Uint8Array;
  timestamp: number;
  sanitizeVersion: number;
}

export class AuthNotImplementedError extends Error {
  constructor() {
    super("auth.verifySignature is a stub — implemented in sub-spec #008");
  }
}

export async function verifySignature(
  _req: Request,
  _env: Env,
  _body: ArrayBuffer,
): Promise<VerifiedRequest> {
  throw new AuthNotImplementedError();
}
