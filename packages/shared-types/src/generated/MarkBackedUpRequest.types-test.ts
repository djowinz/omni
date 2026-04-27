/**
 * Type-binding sidecar for the `identity.markBackedUp` request `params`
 * wire shape per writing-lessons §A8 (contract-oracle coverage).
 *
 * Authoritative oracle (per architectural invariant #21): the host
 * handler `crates/host/src/share/ws_messages.rs::handle_identity_mark_backed_up()`
 * deserialises `params` into an inline anonymous `struct P { path: String,
 * timestamp: u64 }`. There is no exported Rust struct named
 * `MarkBackedUpRequest`; this sidecar declares the wire shape locally as
 * an `interface` and binds it `satisfies`-both-ways so any future drift
 * becomes a `pnpm --filter shared-types typecheck` compile error.
 *
 * Mirrors `2026-04-26-identity-completion-and-display-name §5`
 * (`new section identity.markBackedUp: params { path: "string", timestamp:
 * 1714000000 }`). The host validates `timestamp` is within ±86_400 s of
 * `SystemTime::now`; that's a value-domain rule, not a wire-shape rule, so
 * this sidecar binds `number` only.
 *
 * Note on `timestamp` precision: the wire JSON carries the timestamp as a
 * JSON number. Rust deserialises it into `u64`; JS Number safely
 * represents integers up to 2^53 - 1 (year 287396 in Unix seconds). For
 * the foreseeable horizon a regular `number` is the precise wire shape.
 *
 * If/when T10's host handler is refactored to a proper
 * `#[derive(ts_rs::TS)]` struct, replace the local `interface`
 * declaration with `import type { MarkBackedUpRequest } from
 * './MarkBackedUpRequest'`.
 *
 * This file is intentionally side-effect-only.
 */

interface MarkBackedUpRequest {
  // Filesystem path to the saved backup file. Host validates non-empty.
  path: string;
  // Unix-seconds timestamp the user attests the backup was taken. Host
  // validates ±86_400 s from `SystemTime::now`.
  timestamp: number;
}

// Forward direction — sample literal `satisfies` the interface.
const sample = {
  path: 'C:\\Users\\foo\\identity.omniid',
  timestamp: 1714000000,
} satisfies MarkBackedUpRequest;

// Reverse direction — fresh literal assigned to the type. Adding a field
// without a matching property here fails compilation.
const fresh: MarkBackedUpRequest = {
  path: sample.path,
  timestamp: sample.timestamp,
};

void sample;
void fresh;

// Mark this file as a module so the locally-declared bindings live in
// module scope, not global scope (would otherwise collide with the same
// names in sibling `*.types-test.ts` files under tsc --noEmit).
export {};
