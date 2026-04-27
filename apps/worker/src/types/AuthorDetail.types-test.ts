/**
 * Type-binding sidecar for `AuthorDetail` per writing-lessons §A8
 * (contract-oracle coverage for wire types). Asserts the static
 * `AuthorDetail` interface and the runtime `AuthorDetailSchema` produce
 * the same shape in both directions:
 *
 *   1. Forward — a plain literal of the right shape `satisfies AuthorDetail`.
 *   2. Reverse — `AuthorDetailSchema.parse` output is assignable to
 *      `AuthorDetail`.
 *
 * This file is intentionally side-effect-only and never executed at
 * runtime: importing it from a worker module would bloat the WASM-adjacent
 * bundle, but `tsc --noEmit` (worker `typecheck`) walks it so any drift
 * between the static type and the Zod schema becomes a compile error.
 */
import { AuthorDetailSchema, type AuthorDetail as WorkerAuthorDetail } from '../types';
// Cross-package binding: the public consumer-facing type lives in
// `@omni/shared-types`. The check below proves the worker-local
// Zod-derived type and the consumer-facing interface are structurally
// identical so neither side can drift silently.
import type { AuthorDetail as SharedAuthorDetail } from '@omni/shared-types';

// Forward direction: an inline literal of the right shape `satisfies` the
// worker-local `AuthorDetail`. If a field is renamed, the `satisfies`
// clause fails before any runtime check fires.
const sample = {
  pubkey_hex: 'a'.repeat(64),
  fingerprint_hex: 'a'.repeat(12),
  display_name: 'starfire' as string | null,
  joined_at: 0,
  total_uploads: 0,
} satisfies WorkerAuthorDetail;

// Reverse direction: the runtime parser's output is structurally
// compatible with the worker-local static type.
const parsed: WorkerAuthorDetail = AuthorDetailSchema.parse(sample);

// Cross-package direction: the worker `AuthorDetail` and the
// `@omni/shared-types` `AuthorDetail` describe the same wire shape.
// Both directions of assignability must hold so neither side can silently
// drift (e.g. a field flipped to optional in shared-types).
const sharedFromWorker: SharedAuthorDetail = parsed;
const workerFromShared: WorkerAuthorDetail = sharedFromWorker;

// Touch the names so tsc doesn't flag them as unused under `noUnusedLocals`.
void sample;
void parsed;
void sharedFromWorker;
void workerFromShared;
