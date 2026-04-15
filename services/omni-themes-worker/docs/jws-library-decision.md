# JWS library decision (Task 0 of #008)

**Date:** 2026-04-14
**Decision:** `FALLBACK_WEBCRYPTO`

## Context

Spec #008 (§2) and contract `worker-api.md` §2 originally named
`@tsndr/cloudflare-workers-jwt` (plural `workers`) as the EdDSA JWS
decode + verification library. That package name does not exist on the
npm registry — it returns 404. The real package published by the same
author is **`@tsndr/cloudflare-worker-jwt`** (singular `worker`). The
spec should be treated as having a typo; this doc is the authoritative
record of what was actually evaluated.

## Versions probed

- `@tsndr/cloudflare-worker-jwt@3.2.1` (latest)
- `@noble/ed25519@3.1.0` (latest)
- Node 24.13 (for local probe; the Worker runtime is workerd, which
  exposes the same `crypto.subtle.verify('Ed25519', …)` primitive).

## Probe results

Probe script: `scripts/probe-jws.mjs`.

```
probe1_attached_lib:             false   (library threw "algorithm not found")
probe2a_lib_detached_compact:    false   (library threw "algorithm not found")
probe2b_lib_reconstructed:       false   (library threw "algorithm not found")
probe2c_webcrypto_signingInput:  true    (crypto.subtle.verify('Ed25519',…) succeeded)
```

## Library capability

`@tsndr/cloudflare-worker-jwt@3.2.1` advertises the following `alg`
values in its TypeScript definitions (`index.d.ts:5`):

```
"none" | "ES256" | "ES384" | "ES512" | "HS256" | "HS384" | "HS512" | "RS256" | "RS384" | "RS512"
```

**EdDSA / Ed25519 is not supported at all** — not for attached
payloads, not for detached payloads. The question of whether the
library handles detached JWS (RFC 7515 Appendix F) is moot for our
purposes: it cannot verify the algorithm we require.

## Decision

**Use Web Crypto directly (FALLBACK_WEBCRYPTO).** Task 5 implements
JWS auth as:

1. Split compact form on `.` → `[protected_b64, payload_b64_or_empty, sig_b64]`.
2. Parse `protected_b64` header, assert `alg === 'EdDSA'`, `typ === 'Omni-HTTP-JWS'`.
3. For detached-body contract: reconstruct `payload_b64 = b64url(sha256(body))`
   from the request body bytes ourselves; do not trust the compact's
   payload segment (which per Omni contract is empty).
4. Signing input = `protected_b64 || '.' || payload_b64` (UTF-8 bytes).
5. Import `kid` (base64url pubkey) as `{name:'Ed25519'}` raw key.
6. `crypto.subtle.verify('Ed25519', key, sig_bytes, signing_input_bytes)`.

This is the RFC 7515 §5.2 procedure with the Appendix F detached
adjustment, run directly on the workerd runtime's Web Crypto — no
hand-rolled JWS parser beyond three `split('.')` segments, which is
permissible under writing-lessons rule #16 (simple req + simple
solution + unlikely to expand — the compact form is a closed RFC).

The library is still retained as a dependency because it may be useful
for other JWS shapes (HS256 short-lived admin tokens etc.) later; it
costs <10 KB and removing it again is cheap. If Task 5 ends up never
importing it, the cleanup pass should drop it then.

`@noble/ed25519` is retained for test fixtures (probe + future
integration tests that need to *sign* EdDSA JWS to exercise the
verifier). workerd's Web Crypto exposes `Ed25519` for sign as well,
so production paths should prefer `crypto.subtle`; noble stays in
`dependencies` for now but can be moved to `devDependencies` once
fixture generation consolidates under the WASM fixture harness (Task 3).

## Cleanup (post-decision)

Following the `FALLBACK_WEBCRYPTO` decision above, the cleanup pass:

- **Uninstalled** `@tsndr/cloudflare-worker-jwt` via `pnpm remove`. It
  cannot verify EdDSA, so Task 5 will call `crypto.subtle.verify`
  directly; keeping the dep as "maybe useful for HS256 later" is
  speculative and contradicts writing-lessons #16 (don't keep unused
  deps). If a future task needs HS256 admin tokens, reinstalling is
  one command.
- **Moved `@noble/ed25519` to `devDependencies`.** Production signing
  uses workerd's `crypto.subtle.sign('Ed25519', …)`; noble is only
  imported from test fixtures / the probe script, so shipping it in
  the Worker bundle is unnecessary weight.
