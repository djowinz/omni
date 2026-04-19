// apps/desktop/renderer/lib/share-types.types-test.ts
//
// Bidirectional type-test binding renderer Zod schemas to the ts-rs-
// generated Rust types. Drift in either direction produces a `pnpm
// typecheck` error — this is the enforcement mechanism for spec Pillar 1
// (generated-type contract oracle).
//
// Convention: import generated types from the @omni/shared-types barrel,
// never from ./generated/* directly. See packages/shared-types/src/index.ts
// for the barrel.

import type {
  CachedArtifactDetail as RustCachedArtifactDetail,
  UploadResult as RustUploadResult,
} from '@omni/shared-types';
import { z } from 'zod';
import { CachedArtifactDetailSchema, UploadResultSchema } from './share-types';

// ---- CachedArtifactDetail --------------------------------------------------

// Forward: Zod schema OUTPUT (after parse + defaults) is assignable to the
// Rust shape. Catches Zod adding a field Rust doesn't know about.
type _CachedForward = z.infer<typeof CachedArtifactDetailSchema> extends RustCachedArtifactDetail
  ? true
  : never;

// Reverse: Rust shape is assignable to the Zod schema's INPUT. Catches Rust
// adding a required field Zod doesn't accept.
type _CachedReverse = RustCachedArtifactDetail extends z.input<typeof CachedArtifactDetailSchema>
  ? true
  : never;

// `never` is NOT assignable to `true`, so any broken check fails compile.
const _cachedForward: _CachedForward = true;
const _cachedReverse: _CachedReverse = true;

// ---- UploadResult ----------------------------------------------------------

type _UploadForward = z.infer<typeof UploadResultSchema> extends RustUploadResult ? true : never;
type _UploadReverse = RustUploadResult extends z.input<typeof UploadResultSchema> ? true : never;

const _uploadForward: _UploadForward = true;
const _uploadReverse: _UploadReverse = true;

// Silence "unused" warnings — these declarations exist purely for their
// type-level side effects.
export const __typeTestSentinels = {
  _cachedForward,
  _cachedReverse,
  _uploadForward,
  _uploadReverse,
};
