import { describe, it, expect } from 'vitest';
import { errorResponse, errorFromKind, classifyWasmError } from '../src/lib/errors';
import type { ContentfulStatusCode } from 'hono/utils/http-status';
import type { ErrorBody } from '../src/types';

/**
 * Covers the contract from docs/contracts/worker-api.md §3:
 *  - envelope shape `{ error: { code, message, retry_after? }, kind?, detail? }`
 *  - `errorFromKind` as single source of truth for (kind, detail) → (status, code)
 *  - legacy positional retry-after form for pre-#008 call sites
 */

describe('errorResponse', () => {
  it('produces a Response with status, code, and message (legacy 3-arg)', async () => {
    const res = errorResponse(400 as ContentfulStatusCode, 'BAD_REQUEST', 'bad input');
    expect(res.status).toBe(400);
    expect(res.headers.get('content-type')).toContain('application/json');
    const body = (await res.json()) as ErrorBody;
    expect(body).toEqual({ error: { code: 'BAD_REQUEST', message: 'bad input' } });
  });

  it('accepts legacy number 4th arg as retry_after', async () => {
    const res = errorResponse(429 as ContentfulStatusCode, 'RATE_LIMITED', 'slow down', 60);
    const body = (await res.json()) as ErrorBody;
    expect(body.error.retry_after).toBe(60);
  });

  it('carries kind + detail fields in the envelope', async () => {
    const res = errorResponse(401, 'AUTH_STALE_TIMESTAMP', 'drift > 300s', {
      kind: 'Auth',
      detail: 'StaleTimestamp',
    });
    expect(res.status).toBe(401);
    const body = (await res.json()) as ErrorBody;
    expect(body.error.code).toBe('AUTH_STALE_TIMESTAMP');
    expect(body.kind).toBe('Auth');
    expect(body.detail).toBe('StaleTimestamp');
  });

  it('carries retry_after from options object', async () => {
    const res = errorResponse(429, 'RATE_LIMITED', 'slow', {
      kind: 'Quota',
      detail: 'RateLimited',
      retryAfter: 30,
    });
    const body = (await res.json()) as ErrorBody;
    expect(body.error.retry_after).toBe(30);
    expect(body.kind).toBe('Quota');
  });
});

describe('classifyWasmError', () => {
  const cases: Array<[string, string, string]> = [
    ['rejected executable magic: MZ header', 'Unsafe', 'RejectedExecutableMagic'],
    ['ZipBomb: compression ratio exceeded', 'Unsafe', 'ZipBomb'],
    ['compression bomb', 'Unsafe', 'ZipBomb'],
    ['signature did not verify', 'Integrity', 'SignatureInvalid'],
    ['JWS header decode failed', 'Integrity', 'SignatureInvalid'],
    ['canonical_hash mismatch at file theme.css', 'Integrity', 'HashMismatch'],
    ['sha256 mismatch for manifest file', 'Integrity', 'HashMismatch'],
    ['manifest missing from bundle', 'Integrity', 'ManifestMissing'],
    ['unknown resource kind: wallpaper', 'Malformed', 'UnknownKind'],
    ['UnknownKind', 'Malformed', 'UnknownKind'],
    ['size exceeded: bundle > 5MB', 'Malformed', 'SizeExceeded'],
    ['manifest schema error', 'Malformed', 'ManifestInvalid'],
    ['JSON parse error', 'Malformed', 'ManifestInvalid'],
    ['something completely unexpected', 'Io', 'Generic'],
  ];
  for (const [msg, kind, detail] of cases) {
    it(`classifies "${msg}" → ${kind}/${detail}`, () => {
      const out = classifyWasmError(new Error(msg));
      expect(out.kind).toBe(kind);
      expect(out.detail).toBe(detail);
      expect(out.message).toBe(msg);
    });
  }

  it('accepts a bare string thrown value', () => {
    const out = classifyWasmError('rejected executable magic');
    expect(out.kind).toBe('Unsafe');
    expect(out.detail).toBe('RejectedExecutableMagic');
  });

  it('falls back to Io/Generic on unrepresentable values', () => {
    const out = classifyWasmError({ weird: true });
    expect(out.kind).toBe('Io');
    expect(out.detail).toBe('Generic');
  });
});

describe('errorFromKind — auth category (§3)', () => {
  const cases: Array<[string, ContentfulStatusCode, string]> = [
    ['MalformedEnvelope', 401, 'AUTH_MALFORMED_ENVELOPE'],
    ['UnsupportedAlg', 401, 'AUTH_UNSUPPORTED_ALG'],
    ['MismatchedMethodOrPath', 401, 'AUTH_MISMATCHED_METHOD_OR_PATH'],
    ['BodyOrQueryMismatch', 401, 'AUTH_BODY_OR_QUERY_MISMATCH'],
    ['BadSignature', 401, 'AUTH_BAD_SIGNATURE'],
    ['StaleTimestamp', 401, 'AUTH_STALE_TIMESTAMP'],
    ['UnsupportedVersion', 426, 'AUTH_UNSUPPORTED_VERSION'],
    ['UnknownPubkey', 403, 'UNKNOWN_PUBKEY'],
    ['Forbidden', 403, 'FORBIDDEN'],
  ];
  for (const [detail, status, code] of cases) {
    it(`Auth/${detail} → ${status} ${code}`, async () => {
      const res = errorFromKind('Auth', detail, 'm');
      expect(res.status).toBe(status);
      const body = (await res.json()) as ErrorBody;
      expect(body.error.code).toBe(code);
      expect(body.kind).toBe('Auth');
      expect(body.detail).toBe(detail);
    });
  }
});

describe('errorFromKind — quota category (§3)', () => {
  it('Quota/RateLimited → 429 RATE_LIMITED', async () => {
    const res = errorFromKind('Quota', 'RateLimited', 'slow');
    expect(res.status).toBe(429);
    const body = (await res.json()) as ErrorBody;
    expect(body.error.code).toBe('RATE_LIMITED');
    expect(body.kind).toBe('Quota');
  });
  it('Quota/TurnstileRequired → 428 TURNSTILE_REQUIRED (legacy status)', async () => {
    const res = errorFromKind('Quota', 'TurnstileRequired', 'captcha');
    expect(res.status).toBe(428);
    const body = (await res.json()) as ErrorBody;
    expect(body.error.code).toBe('TURNSTILE_REQUIRED');
  });
  it('Quota/RateLimited threads retryAfter through to error.retry_after', async () => {
    const res = errorFromKind('Quota', 'RateLimited', 'slow', { retryAfter: 42 });
    expect(res.status).toBe(429);
    const body = (await res.json()) as ErrorBody;
    expect(body.error.code).toBe('RATE_LIMITED');
    expect(body.error.retry_after).toBe(42);
  });
  it('Quota/RateLimited without extras has no retry_after field', async () => {
    const res = errorFromKind('Quota', 'RateLimited', 'slow');
    const body = (await res.json()) as ErrorBody;
    expect(body.error.retry_after).toBeUndefined();
  });
  it('non-RATE_LIMITED kinds ignore retryAfter (no stray retry_after)', async () => {
    const res = errorFromKind('Auth', 'BadSignature', 'nope', { retryAfter: 99 });
    const body = (await res.json()) as ErrorBody;
    expect(body.error.retry_after).toBeUndefined();
  });
  it('Quota/TurnstileRequired ignores retryAfter (only RATE_LIMITED threads it)', async () => {
    const res = errorFromKind('Quota', 'TurnstileRequired', 'captcha', { retryAfter: 10 });
    const body = (await res.json()) as ErrorBody;
    expect(body.error.retry_after).toBeUndefined();
  });
});

describe('errorFromKind — malformed category (§3)', () => {
  const cases: Array<[string | undefined, ContentfulStatusCode, string]> = [
    ['BadRequest', 400, 'BAD_REQUEST'],
    ['ManifestInvalid', 400, 'MANIFEST_INVALID'],
    ['SizeExceeded', 413, 'SIZE_EXCEEDED'],
    ['NotFound', 404, 'NOT_FOUND'],
    ['Conflict', 409, 'CONFLICT'],
    [undefined, 400, 'BAD_REQUEST'],
  ];
  for (const [detail, status, code] of cases) {
    it(`Malformed/${String(detail)} → ${status} ${code}`, async () => {
      const res = errorFromKind('Malformed', detail, 'm');
      expect(res.status).toBe(status);
      const body = (await res.json()) as ErrorBody;
      expect(body.error.code).toBe(code);
      expect(body.kind).toBe('Malformed');
    });
  }
});

describe('errorFromKind — unsafe category (§3)', () => {
  it('Unsafe/RejectedExecutableMagic → 422', async () => {
    const res = errorFromKind('Unsafe', 'RejectedExecutableMagic', 'MZ prefix');
    expect(res.status).toBe(422);
    const body = (await res.json()) as ErrorBody;
    expect(body.kind).toBe('Unsafe');
    expect(body.detail).toBe('RejectedExecutableMagic');
  });
  it('Unsafe/CompressionBomb → 422', async () => {
    const res = errorFromKind('Unsafe', 'CompressionBomb', 'ratio');
    expect(res.status).toBe(422);
    const body = (await res.json()) as ErrorBody;
    expect(body.detail).toBe('CompressionBomb');
  });
});

describe('errorFromKind — integrity category (§3)', () => {
  it('Integrity/Tombstoned → 410 TOMBSTONED', async () => {
    const res = errorFromKind('Integrity', 'Tombstoned', 'removed');
    expect(res.status).toBe(410);
    const body = (await res.json()) as ErrorBody;
    expect(body.error.code).toBe('TOMBSTONED');
    expect(body.kind).toBe('Integrity');
  });
  it('Integrity/SchemaVersionUnsupported → 422', async () => {
    const res = errorFromKind('Integrity', 'SchemaVersionUnsupported', 'v99');
    expect(res.status).toBe(422);
    const body = (await res.json()) as ErrorBody;
    expect(body.kind).toBe('Integrity');
    expect(body.detail).toBe('SchemaVersionUnsupported');
  });
});

describe('errorFromKind — admin category (§3)', () => {
  const cases: Array<[string, ContentfulStatusCode, string]> = [
    ['NotModerator', 403, 'ADMIN_NOT_MODERATOR'],
    ['BadTag', 400, 'ADMIN_BAD_TAG'],
    // Spec §9b + plan W3T12: this is a conflict with existing resource state → 409.
    ['WouldOrphanArtifacts', 409, 'ADMIN_WOULD_ORPHAN_ARTIFACTS'],
    ['BadValue', 400, 'ADMIN_BAD_VALUE'],
    ['NoOp', 400, 'ADMIN_NO_OP'],
  ];
  for (const [detail, status, code] of cases) {
    it(`Admin/${detail} → ${status} ${code}`, async () => {
      const res = errorFromKind('Admin', detail, 'm');
      expect(res.status).toBe(status);
      const body = (await res.json()) as ErrorBody;
      expect(body.error.code).toBe(code);
      expect(body.kind).toBe('Admin');
    });
  }
});

describe('errorFromKind — io category (§3)', () => {
  it('Io/* → 500 SERVER_ERROR', async () => {
    const res = errorFromKind('Io', undefined, 'boom');
    expect(res.status).toBe(500);
    const body = (await res.json()) as ErrorBody;
    expect(body.error.code).toBe('SERVER_ERROR');
    expect(body.kind).toBe('Io');
  });
});
