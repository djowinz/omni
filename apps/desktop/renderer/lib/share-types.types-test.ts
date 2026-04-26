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
  ModerationCheckResult as RustModerationCheckResult,
  PackProgress as RustPackProgress,
  PublishablesEntry as RustPublishablesEntry,
  PublishSidecar as RustPublishSidecar,
  UploadResult as RustUploadResult,
} from '@omni/shared-types';
import type { z } from 'zod';
import type {
  CachedArtifactDetailSchema,
  ModerationCheckResultSchema,
  PackProgressSchema,
  PublishablesEntrySchema,
  PublishSidecarSchema,
  UploadResultSchema,
} from './share-types';

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

// ---- PackProgress (upload-flow-redesign §8.8) ------------------------------
//
// Shipped Rust source: crates/host/src/share/ws_messages.rs PackProgress
// (with PackStage + StageStatus enums riding through the same wire frame).
// Generated TS: packages/shared-types/src/generated/PackProgress.ts.
//
// Forward + reverse assignability ensures the renderer's Zod schema and the
// host's emitted JSON stay byte-identical at the type level.

type _PackProgressForward = z.infer<typeof PackProgressSchema> extends RustPackProgress
  ? true
  : never;
type _PackProgressReverse = RustPackProgress extends z.input<typeof PackProgressSchema>
  ? true
  : never;

const _packProgressForward: _PackProgressForward = true;
const _packProgressReverse: _PackProgressReverse = true;

// ---- PublishSidecar (upload-flow-redesign §8.1) ----------------------------

type _SidecarForward = z.infer<typeof PublishSidecarSchema> extends RustPublishSidecar
  ? true
  : never;
type _SidecarReverse = RustPublishSidecar extends z.input<typeof PublishSidecarSchema>
  ? true
  : never;

const _sidecarForward: _SidecarForward = true;
const _sidecarReverse: _SidecarReverse = true;

// ---- PublishablesEntry (upload-flow-redesign §8.8 + INV-7.1.10) ------------

type _PublishablesForward = z.infer<typeof PublishablesEntrySchema> extends RustPublishablesEntry
  ? true
  : never;
type _PublishablesReverse = RustPublishablesEntry extends z.input<typeof PublishablesEntrySchema>
  ? true
  : never;

const _publishablesForward: _PublishablesForward = true;
const _publishablesReverse: _PublishablesReverse = true;

// ---- ModerationCheckResult (upload-flow-redesign §7.7) ---------------------
//
// Shipped Rust source: crates/host/src/share/ws_messages.rs ModerationCheckResult
// Generated TS: packages/shared-types/src/generated/ModerationCheckResult.ts
//
// The Zod payload schema (the inner `params`) binds bidirectionally to the
// generated Rust shape. The outer envelope schema
// (ShareModerationCheckResultSchema) wraps the same payload in the standard
// `{ id, type, params }` frame and is validated by `useShareWs.send` at
// runtime; the frame shape is local to the WS protocol and not part of the
// cross-boundary type contract.

type _ModerationForward = z.infer<typeof ModerationCheckResultSchema> extends RustModerationCheckResult
  ? true
  : never;
type _ModerationReverse = RustModerationCheckResult extends z.input<
  typeof ModerationCheckResultSchema
>
  ? true
  : never;

const _moderationForward: _ModerationForward = true;
const _moderationReverse: _ModerationReverse = true;

// Silence "unused" warnings — these declarations exist purely for their
// type-level side effects.
export const __typeTestSentinels = {
  _cachedForward,
  _cachedReverse,
  _uploadForward,
  _uploadReverse,
  _packProgressForward,
  _packProgressReverse,
  _sidecarForward,
  _sidecarReverse,
  _publishablesForward,
  _publishablesReverse,
  _moderationForward,
  _moderationReverse,
};
