/**
 * Type-binding sidecar for the `identity.setDisplayName` request `params`
 * wire shape per writing-lessons §A8 (contract-oracle coverage).
 *
 * Authoritative oracle (per architectural invariant #21): the host
 * handler `crates/host/src/share/ws_messages.rs::handle_identity_set_display_name()`
 * deserialises `params` into an inline anonymous `struct P { display_name:
 * String }`. There is no exported Rust struct named
 * `SetDisplayNameRequest`; this sidecar declares the wire shape locally
 * as an `interface` and binds it `satisfies`-both-ways so any future
 * drift becomes a `pnpm --filter shared-types typecheck` compile error.
 *
 * Mirrors `2026-04-26-identity-completion-and-display-name §5` (`new
 * section identity.setDisplayName: params { display_name: "starfire" }`).
 * The host validates `display_name` per spec §3.4 (NFC-normalize, trim,
 * 1..=32 Unicode code points, no controls, no surrogates) — those are
 * value-domain rules that don't change the wire shape; this sidecar
 * binds the field as `string`.
 *
 * If/when T10's host handler is refactored to a proper
 * `#[derive(ts_rs::TS)]` struct, replace the local `interface`
 * declaration with `import type { SetDisplayNameRequest } from
 * './SetDisplayNameRequest'`.
 *
 * This file is intentionally side-effect-only.
 */

interface SetDisplayNameRequest {
  display_name: string;
}

// Forward direction — sample literal `satisfies` the interface.
const sample = {
  display_name: 'starfire',
} satisfies SetDisplayNameRequest;

// Reverse direction — fresh literal assigned to the type. Adding a field
// without a matching property here fails compilation.
const fresh: SetDisplayNameRequest = {
  display_name: sample.display_name,
};

void sample;
void fresh;

// Mark this file as a module so the locally-declared bindings live in
// module scope, not global scope (would otherwise collide with the same
// names in sibling `*.types-test.ts` files under tsc --noEmit).
export {};
