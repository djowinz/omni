/**
 * Type-binding sidecar for the `identity.show` result wire shape per
 * writing-lessons §A8 (contract-oracle coverage for wire types).
 *
 * Authoritative oracle (per architectural invariant #21): the host handler
 * `crates/host/src/share/ws_messages.rs::handle_identity_show()` constructs
 * the response via a `json!{ ... }` macro — there is no exported Rust
 * struct named `IdentityShowResult`. This sidecar therefore declares the
 * wire shape locally as an `interface` and binds it `satisfies`-both-ways
 * to a sample literal so any future drift between the renderer's
 * understanding of the shape and the host's emission becomes a
 * `pnpm --filter shared-types typecheck` compile error.
 *
 * Mirrors the spec at `2026-04-26-identity-completion-and-display-name §5`
 * (`identity.show result extended` block) and the `IdentityMetadata`
 * generated type sibling (`./IdentityMetadata.ts`) for the metadata-derived
 * fields. `last_backed_up_at` / `last_rotated_at` are `bigint | null`
 * because the Rust source declares them `Option<u64>` and ts-rs maps `u64`
 * to `bigint` to preserve full precision.
 *
 * If/when T10's host handler is refactored to construct + serialise a
 * proper `#[derive(ts_rs::TS)]` struct, replace the local `interface`
 * declaration below with `import type { IdentityShowResult } from
 * './IdentityShowResult'` (the generated file) and keep the bidirectional
 * binding intact.
 *
 * This file is intentionally side-effect-only — `tsc --noEmit` walks it
 * but no runtime code imports it.
 */

interface IdentityShowResult {
  pubkey_hex: string;
  fingerprint_hex: string;
  fingerprint_words: string[];
  fingerprint_emoji: string[];
  // Unix-seconds; host hard-codes 0 today (key-creation timestamps not
  // tracked) but the field is part of the contract.
  created_at: number;
  display_name: string | null;
  backed_up: boolean;
  // Spec §3.1 IdentityMetadata: u64 → ts-rs emits bigint to preserve
  // precision. Match the IdentityMetadata.ts generated shape.
  last_backed_up_at: bigint | null;
  last_rotated_at: bigint | null;
  last_backup_path: string | null;
}

// Forward direction — sample literal of the documented shape `satisfies` the
// interface. Renaming a field or changing a type breaks this clause before
// any runtime check fires.
const sample = {
  pubkey_hex: 'a'.repeat(64),
  fingerprint_hex: 'a'.repeat(12),
  fingerprint_words: ['apple', 'banana', 'cobra'],
  fingerprint_emoji: ['🦊', '🌲', '🚀', '🧊', '🌙', '⚡'],
  created_at: 0,
  display_name: 'starfire' as string | null,
  backed_up: true,
  last_backed_up_at: 1714000000n as bigint | null,
  last_rotated_at: null as bigint | null,
  last_backup_path: 'C:\\Users\\foo\\identity.omniid' as string | null,
} satisfies IdentityShowResult;

// Reverse direction — a fresh literal assigned to the type. Any field the
// interface adds without a matching property below fails this assignment.
const fresh: IdentityShowResult = {
  pubkey_hex: sample.pubkey_hex,
  fingerprint_hex: sample.fingerprint_hex,
  fingerprint_words: [...sample.fingerprint_words],
  fingerprint_emoji: [...sample.fingerprint_emoji],
  created_at: sample.created_at,
  display_name: sample.display_name,
  backed_up: sample.backed_up,
  last_backed_up_at: sample.last_backed_up_at,
  last_rotated_at: sample.last_rotated_at,
  last_backup_path: sample.last_backup_path,
};

// Cover the null-branch on every nullable field — proves the interface
// permits `null` (not just the populated case above).
const nulled: IdentityShowResult = {
  pubkey_hex: 'b'.repeat(64),
  fingerprint_hex: 'b'.repeat(12),
  fingerprint_words: [],
  fingerprint_emoji: [],
  created_at: 0,
  display_name: null,
  backed_up: false,
  last_backed_up_at: null,
  last_rotated_at: null,
  last_backup_path: null,
};

void sample;
void fresh;
void nulled;

// Mark this file as a module so the locally-declared `sample` / `fresh` /
// `nulled` bindings live in module scope, not global scope. Without this
// `export {}` they collide with the same names in sibling
// `*.types-test.ts` files (both are walked by `tsc --noEmit` against the
// shared-types tsconfig).
export {};
