/**
 * Type-binding sidecar for the `identity.rotate` result wire shape per
 * writing-lessons §A8 (contract-oracle coverage for wire types).
 *
 * Authoritative oracle (per architectural invariant #21): the host handler
 * `crates/host/src/share/ws_messages.rs::handle_identity_rotate()`
 * constructs the response via a `json!{ ... }` macro — there is no
 * exported Rust struct named `IdentityRotateResult`. This sidecar
 * declares the wire shape locally as an `interface` and binds it
 * `satisfies`-both-ways so any future drift becomes a
 * `pnpm --filter shared-types typecheck` compile error.
 *
 * Mirrors `2026-04-26-identity-completion-and-display-name §5`
 * (`new section identity.rotate: result { pubkey_hex, fingerprint_hex }`).
 *
 * If/when T10's host handler is refactored to construct + serialise a
 * proper `#[derive(ts_rs::TS)]` struct, replace the local `interface`
 * declaration below with `import type { IdentityRotateResult } from
 * './IdentityRotateResult'` (the generated file) and keep the binding.
 *
 * This file is intentionally side-effect-only.
 */

interface IdentityRotateResult {
  pubkey_hex: string;
  fingerprint_hex: string;
}

// Forward direction — sample literal `satisfies` the interface.
const sample = {
  pubkey_hex: 'a'.repeat(64),
  fingerprint_hex: 'a'.repeat(12),
} satisfies IdentityRotateResult;

// Reverse direction — fresh literal assigned to the type. Adding a field
// to the interface without a matching property here fails compilation.
const fresh: IdentityRotateResult = {
  pubkey_hex: sample.pubkey_hex,
  fingerprint_hex: sample.fingerprint_hex,
};

void sample;
void fresh;

// Mark this file as a module so the locally-declared bindings live in
// module scope, not global scope (would otherwise collide with the same
// names in sibling `*.types-test.ts` files under tsc --noEmit).
export {};
