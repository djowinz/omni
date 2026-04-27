/**
 * share-types.ts — Wire-type registry for the host ⇄ renderer WebSocket share surface.
 *
 * Every type carries dual oracle-comments per architectural invariant #21:
 *   // Oracle: contracts/ws-explorer.md §<section>
 *   // Shipped: crates/host/src/share/<file>.rs <Struct/fn>
 *
 * Shipped Rust code is the authority when it disagrees with the contract doc
 * (invariant #21). All drift from the contract is noted inline.
 *
 * Error envelope (D-004-J): every error frame is
 *   { id, type: "error", error: { code, kind, detail, message } }
 * Renderers MUST render `message`; they MUST NOT parse `detail`.
 */

import { z } from 'zod';

// Re-export the D-004-J error envelope as ShareWsError. Consumers who need the
// underlying OmniError type can import it directly from
// `./map-error-to-user-message`.
export type { OmniError as ShareWsError } from './map-error-to-user-message';

// ── Error envelope ────────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §Error codes
// Shipped: crates/host/src/share/handlers.rs ErrorPayload + error_frame()
//          crates/host/src/share/progress.rs error_envelope()
export const ShareErrorSchema = z.object({
  code: z.string(),
  kind: z.enum(['Malformed', 'Unsafe', 'Integrity', 'Io', 'Auth', 'Quota', 'Admin', 'HostLocal']),
  detail: z.string().nullable().optional(),
  message: z.string(),
  // `UploadError::DependencyViolations` carries a structured per-violation
  // array on the wire envelope (host emits it via `progress.rs::error_envelope`
  // alongside code/kind/detail/message). Each entry has `kind` (`missing-ref`,
  // `unused-file`, `content-safety`), `path`, optional `detail` string, and
  // optional `confidence` (content-safety only). Kept loose-typed here so the
  // schema doesn't fight forward-compat additions; the renderer's
  // `extractPackViolations` validates entries via its own predicate before use.
  violations: z.array(z.record(z.string(), z.unknown())).optional(),
});
export type ShareError = z.infer<typeof ShareErrorSchema>;

export const ShareErrorFrameSchema = z.object({
  id: z.string(),
  type: z.literal('error'),
  error: ShareErrorSchema,
});
export type ShareErrorFrame = z.infer<typeof ShareErrorFrameSchema>;

// ── Install phase enums ───────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §explorer.install progress
// Shipped: crates/host/src/share/handlers.rs install_progress_to_contract_frame()
//          crates/host/src/share/install.rs InstallProgress (5 Rust variants → 4 wire phases)
//
// Wire phases are the 4 values the host actually emits over the WebSocket.
// The Rust InstallProgress enum has 5 variants: Downloading, Verifying,
// Sanitizing, Writing, Committing — both Writing and Committing map to "write".
export const WireInstallPhaseSchema = z.enum(['download', 'verify', 'sanitize', 'write']);
export type WireInstallPhase = z.infer<typeof WireInstallPhaseSchema>;

// Component-facing install phase adds terminal pseudo-phases used locally by
// <InstallProgress /> — "done" and "error" are never emitted by the host.
export const InstallPhaseSchema = z.enum([
  'download',
  'verify',
  'sanitize',
  'write',
  'done',
  'error',
]);
export type InstallPhase = z.infer<typeof InstallPhaseSchema>;

// ── CachedArtifactDetail (explorer.list items) ────────────────────────────────

// Oracle: contracts/ws-explorer.md §explorer.list result items
// Shipped: crates/host/src/share/cache.rs CachedArtifactDetail
// Worker source: apps/worker/src/routes/list.ts rowToItem()
//
// The worker's /v1/list response does NOT include `r2_url` — that URL is
// served by /v1/get (see ArtifactDetailSchema below) so consumers fetch
// detail on selection. `r2_url` is optional here to keep this schema
// dual-use as the installed-bundle cache descriptor, where r2_url is
// populated from the install-time /v1/get response.
//
// Added fields (tags, installs, author_fingerprint_hex, created_at) reflect
// what the worker actually sends today — prior versions of this schema
// discarded them silently via z.object()'s default unknown-key stripping.
//
// Shape alignment note (integration-testing-discipline #A2):
// All previously-optional fields now use .default() so the inferred type
// always carries the field (never undefined). This matches the Rust struct's
// #[serde(default)] which guarantees a value is always present. The generated
// ts-rs type (packages/shared-types/src/generated/CachedArtifactDetail.ts)
// treats all fields as required; the sidecar type-test enforces bidirectional
// assignability between this schema and the generated type.
// `kind` uses z.string() rather than z.enum() — the Rust source is String (not
// a closed enum), and the generated type is `string`. The enum constraint is
// enforced at runtime by the worker, not at the type level here.
export const CachedArtifactDetailSchema = z.object({
  artifact_id: z.string(),
  content_hash: z.string(),
  author_pubkey: z.string(),
  // Default to empty string — matches Rust #[serde(default)] on this field.
  author_fingerprint_hex: z.string().default(''),
  name: z.string(),
  // z.string() (not z.enum()) — Rust source is String; generated TS is string.
  // Runtime values are always "theme" | "bundle" per the worker contract.
  kind: z.string(),
  tags: z.array(z.string()).default([]),
  installs: z.number().int().default(0),
  // Default to empty string — matches Rust #[serde(default)] on this field.
  r2_url: z.string().default(''),
  thumbnail_url: z.string(),
  // Default to 0 — matches Rust #[serde(default)] on this field.
  created_at: z.number().int().default(0),
  updated_at: z.number().int(),
});
export type CachedArtifactDetail = z.infer<typeof CachedArtifactDetailSchema>;

// ── ArtifactDetail (explorer.get artifact) ────────────────────────────────────

// Oracle: contracts/ws-explorer.md §explorer.get result
// Shipped: crates/host/src/share/client.rs ArtifactDetail (worker-api §4.4)
export const ArtifactDetailSchema = z.object({
  artifact_id: z.string(),
  // Intentionally z.string() (not enum) — worker API may emit future kinds not
  // yet known to the host cache layer (e.g. 'preset'). Do not tighten to enum.
  kind: z.string(),
  manifest: z.record(z.string(), z.unknown()),
  content_hash: z.string(),
  r2_url: z.string(),
  thumbnail_url: z.string(),
  author_pubkey: z.string(),
  author_fingerprint_hex: z.string(),
  installs: z.number().int().default(0),
  reports: z.number().int().default(0),
  created_at: z.number().int().default(0),
  updated_at: z.number().int().default(0),
  status: z.string(),
});
export type ArtifactDetail = z.infer<typeof ArtifactDetailSchema>;

// ── UploadResult (upload.publish direct result) ────────────────────────────────

// Oracle: crates/host/src/share/upload.rs UploadResult + UploadStatus
// Generated TS oracle: packages/shared-types/src/generated/UploadResult.ts
//                      packages/shared-types/src/generated/UploadStatus.ts
//
// This is the struct the host emits as the direct return value of the
// upload pipeline. `pump_to_ws` in crates/host/src/share/progress.rs wraps
// it in a WS frame with a remapped shape (UploadPublishResultSchema below);
// this schema stays useful as a typed oracle for any future consumer that
// wants the raw result shape, and as the bidirectional type-test anchor in
// share-types.types-test.ts.
export const UploadStatusSchema = z.enum(['created', 'deduplicated', 'updated', 'unchanged']);
export type UploadStatus = z.infer<typeof UploadStatusSchema>;

export const UploadResultSchema = z.object({
  artifact_id: z.string(),
  content_hash: z.string(),
  r2_url: z.string(),
  thumbnail_url: z.string(),
  status: UploadStatusSchema,
});
export type UploadResult = z.infer<typeof UploadResultSchema>;

// ─────────────────────────────────────────────────────────────────────────────
// explorer.* request params + response shapes
// ─────────────────────────────────────────────────────────────────────────────

// ── explorer.list ──────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §explorer.list
// Shipped: crates/host/src/share/ws_messages.rs handle_list() struct P
export const ExplorerListParamsSchema = z.object({
  kind: z.enum(['theme', 'bundle', 'all']).optional(),
  sort: z.enum(['new', 'installs', 'name']).optional(),
  tags: z.array(z.string()).optional(),
  cursor: z.string().nullable().optional(),
  limit: z.number().int().positive().optional(),
  // Optional 64-hex author pubkey filter. When set, the worker returns
  // only that author's artifacts. Consumed by My Uploads (#015) which
  // passes the editor's own pubkey to show the current user's uploads.
  author_pubkey: z
    .string()
    .regex(/^[0-9a-fA-F]{64}$/)
    .optional(),
});
export type ExplorerListParams = z.infer<typeof ExplorerListParamsSchema>;

// Oracle: contracts/ws-explorer.md §explorer.list result
// Shipped: crates/host/src/share/ws_messages.rs handle_list() — emits items + next_cursor
export const ExplorerListResultSchema = z.object({
  id: z.string(),
  type: z.literal('explorer.listResult'),
  items: z.array(CachedArtifactDetailSchema),
  next_cursor: z.string().nullable(),
});
export type ExplorerListResult = z.infer<typeof ExplorerListResultSchema>;

// ── explorer.get ───────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §explorer.get
// Shipped: crates/host/src/share/ws_messages.rs handle_get() struct P
export const ExplorerGetParamsSchema = z.object({
  artifact_id: z.string(),
});
export type ExplorerGetParams = z.infer<typeof ExplorerGetParamsSchema>;

// Oracle: contracts/ws-explorer.md §explorer.get result
// Shipped: crates/host/src/share/ws_messages.rs handle_get() — emits artifact: ArtifactDetail
export const ExplorerGetResultSchema = z.object({
  id: z.string(),
  type: z.literal('explorer.getResult'),
  artifact: ArtifactDetailSchema,
});
export type ExplorerGetResult = z.infer<typeof ExplorerGetResultSchema>;

// ── explorer.install ───────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §explorer.install params
// Shipped: crates/host/src/share/ws_messages.rs handle_install() struct P
//          crates/host/src/share/install.rs InstallRequest
//
// DRIFT NOTE: The ws_messages.rs handler parses the field as
// `expected_fingerprint_hex` but immediately errors if it is present
// ("expected_fingerprint_hex pinning is not yet supported; omit the field").
// The contract (ws-explorer.md) names this `expected_pubkey_hex` (64-hex,
// full Ed25519 pubkey). Per invariant #21 shipped code is the oracle — the
// shipped InstallRequest carries `expected_pubkey: Option<PublicKey>` and is
// always set to `None` until INV23 dispatcher fix (task #14). The renderer
// should send `expected_pubkey_hex` per the contract; the host's current
// dispatcher will error on either form. This type reflects the contract's
// intended shape — always omit the field or pass null until #14 lands.
export const ExplorerInstallParamsSchema = z.object({
  artifact_id: z.string(),
  target_workspace: z.string().optional(),
  overwrite: z.boolean().optional(),
  // 64-hex Ed25519 pubkey or null. NOT a fingerprint. Per ws-explorer.md §explorer.install.
  // Shipped InstallRequest.expected_pubkey: Option<PublicKey>.
  expected_pubkey_hex: z
    .string()
    .regex(/^[0-9a-fA-F]{64}$/)
    .nullable()
    .optional(),
});
export type ExplorerInstallParams = z.infer<typeof ExplorerInstallParamsSchema>;

// Oracle: contracts/ws-explorer.md §explorer.install progress
// Shipped: crates/host/src/share/handlers.rs install_progress_to_contract_frame()
export const ExplorerInstallProgressSchema = z.object({
  id: z.string(),
  type: z.literal('explorer.installProgress'),
  phase: WireInstallPhaseSchema,
  done: z.number().int().nonnegative(),
  total: z.number().int().nonnegative(),
});
export type ExplorerInstallProgress = z.infer<typeof ExplorerInstallProgressSchema>;

// Oracle: contracts/ws-explorer.md §explorer.install result
// Shipped: crates/host/src/share/handlers.rs install_outcome_to_result_frame()
//          crates/host/src/share/install.rs InstallOutcome
export const ExplorerInstallResultSchema = z.object({
  id: z.string(),
  type: z.literal('explorer.installResult'),
  installed_path: z.string(),
  content_hash: z.string(),
  author_fingerprint_hex: z.string(),
  tofu: z.enum(['first_install', 'matched', 'mismatch']),
  warnings: z.array(z.string()),
});
export type ExplorerInstallResult = z.infer<typeof ExplorerInstallResultSchema>;

// ── explorer.preview ───────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §explorer.preview
// Shipped: crates/host/src/share/ws_messages.rs handle_preview() struct P
export const ExplorerPreviewParamsSchema = z.object({
  artifact_id: z.string(),
});
export type ExplorerPreviewParams = z.infer<typeof ExplorerPreviewParamsSchema>;

// Oracle: contracts/ws-explorer.md §explorer.preview result: { preview_token: "uuid" }
// Shipped: crates/host/src/share/ws_messages.rs handle_preview() — emits preview_token
// DRIFT NOTE: the host emits preview_token at the top level of the frame
// (not nested under "params"), e.g. { id, type, preview_token }
export const ExplorerPreviewResultSchema = z.object({
  id: z.string(),
  type: z.literal('explorer.previewResult'),
  preview_token: z.string().uuid(),
});
export type ExplorerPreviewResult = z.infer<typeof ExplorerPreviewResultSchema>;

// ── explorer.cancelPreview ─────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §explorer.cancelPreview params
// Shipped: crates/host/src/share/ws_messages.rs handle_cancel_preview() struct P
export const ExplorerCancelPreviewParamsSchema = z.object({
  preview_token: z.string().uuid(),
});
export type ExplorerCancelPreviewParams = z.infer<typeof ExplorerCancelPreviewParamsSchema>;

// Oracle: contracts/ws-explorer.md §explorer.cancelPreview result: { restored: true }
// Shipped: crates/host/src/share/ws_messages.rs handle_cancel_preview() — emits restored: true
// DRIFT NOTE: host emits restored at top level of the frame, not nested under "params"
export const ExplorerCancelPreviewResultSchema = z.object({
  id: z.string(),
  type: z.literal('explorer.cancelPreviewResult'),
  restored: z.literal(true),
});
export type ExplorerCancelPreviewResult = z.infer<typeof ExplorerCancelPreviewResultSchema>;

// ── explorer.fork ──────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §explorer.fork params
// Shipped: crates/host/src/share/ — no fork.rs present in shipped codebase;
//          dispatch arm not present in ws_messages.rs. Type reflects contract shape.
export const ExplorerForkParamsSchema = z.object({
  artifact_id: z.string(),
  target_name: z.string(),
});
export type ExplorerForkParams = z.infer<typeof ExplorerForkParamsSchema>;

// Oracle: contracts/ws-explorer.md §explorer.fork result
// Shipped: no ws_messages.rs dispatch arm exists yet — #016 adds the fork handler. Shape derived from contract only.
export const ExplorerForkResultSchema = z.object({
  id: z.string(),
  type: z.literal('explorer.forkResult'),
  workspace_path: z.string(),
  new_manifest: z.record(z.string(), z.unknown()),
});
export type ExplorerForkResult = z.infer<typeof ExplorerForkResultSchema>;

// ─────────────────────────────────────────────────────────────────────────────
// upload.* request params + response shapes
// ─────────────────────────────────────────────────────────────────────────────

// ── upload.pack ─────────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §upload.pack params
// Shipped: crates/host/src/share/ws_messages.rs handle_pack() struct P
export const UploadPackParamsSchema = z.object({
  workspace_path: z.string(),
  kind: z.enum(['theme', 'bundle']).optional(),
  name: z.string().optional(),
});
export type UploadPackParams = z.infer<typeof UploadPackParamsSchema>;

// Oracle: contracts/ws-explorer.md §upload.pack result
// Shipped: crates/host/src/share/ws_messages.rs handle_pack() — emits packResult frame
export const UploadPackResultSchema = z.object({
  id: z.string(),
  type: z.literal('upload.packResult'),
  params: z.object({
    content_hash: z.string(),
    compressed_size: z.number().int().nonnegative(),
    uncompressed_size: z.number().int().nonnegative(),
    manifest: z.record(z.string(), z.unknown()),
    sanitize_report: z.record(z.string(), z.unknown()),
  }),
});
export type UploadPackResult = z.infer<typeof UploadPackResultSchema>;

// Oracle: docs/superpowers/specs/2026-04-21-upload-flow-redesign-design.md §8.8
// Shipped: crates/host/src/share/ws_messages.rs PackProgress + PackStage + StageStatus
// Generated TS oracle:
//   packages/shared-types/src/generated/PackProgress.ts
//   packages/shared-types/src/generated/PackStage.ts
//   packages/shared-types/src/generated/StageStatus.ts
//
// Replaces the legacy `phase/done/total` pack-progress shape (which never
// shipped a real emitter — spec §8.8 supersedes it). The Wave A1 pack
// pipeline emits one frame per pack stage (schema → content-safety → asset
// → dependency → size) with running/passed/failed status; the renderer's
// progressive Step 3 UI accumulates these into a per-stage record.
//
// `params` IS the `PackProgress` Rust struct (the host wraps it in the
// standard `{ id, type, params }` envelope at emit time). The kebab-case
// stage values mirror serde's rename_all; the lowercase status values mirror
// serde's rename_all = "lowercase".
export const PackProgressSchema = z.object({
  stage: z.enum(['schema', 'content-safety', 'asset', 'dependency', 'size']),
  status: z.enum(['running', 'passed', 'failed']),
  // ts-rs emits `Option<String>` as `string | null` (not `?:`), so the Zod
  // schema mirrors that exact wire shape. Required-but-nullable matches the
  // bidirectional types-test in share-types.types-test.ts.
  detail: z.string().nullable(),
});
export type PackProgress = z.infer<typeof PackProgressSchema>;

export const UploadPackProgressSchema = z.object({
  id: z.string(),
  type: z.literal('upload.packProgress'),
  params: PackProgressSchema,
});
export type UploadPackProgress = z.infer<typeof UploadPackProgressSchema>;

// ── upload.publish ──────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §upload.publish params
// Shipped: crates/host/src/share/ws_messages.rs handle_publish() struct P
export const UploadPublishParamsSchema = z.object({
  workspace_path: z.string(),
  visibility: z.literal('public'),
  bump: z.enum(['patch', 'minor', 'major', 'none']),
  kind: z.enum(['theme', 'bundle']).optional(),
  name: z.string().optional(),
  description: z.string().optional(),
  tags: z.array(z.string()).optional(),
  license: z.string().optional(),
  version: z.string().optional(),
  omni_min_version: z.string().optional(),
});
export type UploadPublishParams = z.infer<typeof UploadPublishParamsSchema>;

// Oracle: contracts/ws-explorer.md §upload.publishProgress
// Shipped: crates/host/src/share/progress.rs pump_to_ws() (emits `{ id, type, params: { phase, done, total } }`; WireProgress lives under params)
//          phase ∈ {"pack", "sanitize", "upload"}
export const UploadPublishProgressSchema = z.object({
  id: z.string(),
  type: z.literal('upload.publishProgress'),
  params: z.object({
    phase: z.enum(['pack', 'sanitize', 'upload']),
    done: z.number().int().nonnegative(),
    total: z.number().int().nonnegative(),
  }),
});
export type UploadPublishProgress = z.infer<typeof UploadPublishProgressSchema>;

// Oracle: contracts/ws-explorer.md §upload.publish result
// Shipped: crates/host/src/share/progress.rs pump_to_ws() (emits `{ id, type: "upload.publishResult", params: { artifact_id, content_hash, status, worker_url } }`)
//          crates/host/src/share/upload.rs UploadResult + UploadStatus
export const UploadPublishResultSchema = z.object({
  id: z.string(),
  type: z.literal('upload.publishResult'),
  params: z.object({
    artifact_id: z.string(),
    content_hash: z.string(),
    status: z.enum(['created', 'deduplicated']),
    worker_url: z.string(),
  }),
});
export type UploadPublishResult = z.infer<typeof UploadPublishResultSchema>;

// ── upload.update ───────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §upload.update params
// Shipped: crates/host/src/share/ws_messages.rs handle_publish(is_update=true) struct P
export const UploadUpdateParamsSchema = z.object({
  artifact_id: z.string(),
  workspace_path: z.string(),
  bump: z.enum(['patch', 'minor', 'major', 'none']),
  kind: z.enum(['theme', 'bundle']).optional(),
  name: z.string().optional(),
  description: z.string().optional(),
  tags: z.array(z.string()).optional(),
  license: z.string().optional(),
  version: z.string().optional(),
  omni_min_version: z.string().optional(),
});
export type UploadUpdateParams = z.infer<typeof UploadUpdateParamsSchema>;

// Oracle: contracts/ws-explorer.md §upload.update result — same as upload.publish
// Shipped: crates/host/src/share/progress.rs pump_to_ws() (emits `{ id, type: "upload.updateResult", params: { artifact_id, content_hash, status, worker_url } }`)
export const UploadUpdateResultSchema = z.object({
  id: z.string(),
  type: z.literal('upload.updateResult'),
  params: z.object({
    artifact_id: z.string(),
    content_hash: z.string(),
    status: z.enum(['created', 'deduplicated', 'updated', 'unchanged']),
    worker_url: z.string(),
  }),
});
export type UploadUpdateResult = z.infer<typeof UploadUpdateResultSchema>;

// ── share.moderationCheck ───────────────────────────────────────────────────

// Oracle: docs/superpowers/specs/2026-04-21-upload-flow-redesign-design.md §7.7
// Shipped: crates/host/src/share/ws_messages.rs handle_moderation_check + ModerationCheckResult
// Generated TS oracle: packages/shared-types/src/generated/ModerationCheckResult.ts
//
// Renderer-initiated single-image moderation gate (INV-7.7.2 site #1). The
// renderer base64-encodes the user-uploaded Preview Image bytes and sends
// them to the host; the host runs the bundled NudeNet ONNX detector via
// `share::moderation::check_image` and returns the precomputed rejection
// flag (INV-7.7.3 threshold = 0.8 applied host-side). The renderer never
// reapplies the threshold — it switches into the amber rejection state on
// `rejected: true` per INV-7.7.4/INV-7.7.5.
//
// `unsafe_score` + `label` are surfaced for INV-7.7.6's collapsible detail
// block (`code Moderation:ClientRejected · detector onnx-nudenet-v1 ·
// confidence 0.XX`). Sidecar types-test in `share-types.types-test.ts`
// binds this Zod payload bidirectionally to `RustModerationCheckResult`.
export const ModerationCheckResultSchema = z.object({
  unsafe_score: z.number(),
  label: z.string(),
  rejected: z.boolean(),
});
export type ModerationCheckResult = z.infer<typeof ModerationCheckResultSchema>;

export const ShareModerationCheckParamsSchema = z.object({
  image_base64: z.string(),
});
export type ShareModerationCheckParams = z.infer<typeof ShareModerationCheckParamsSchema>;

// Result frame envelope shape — `{ id, type, params: ModerationCheckResult }`
// matching the host's `json!({ "id", "type": "share.moderationCheckResult",
// "params": <result> })` emit path.
export const ShareModerationCheckResultSchema = z.object({
  id: z.string(),
  type: z.literal('share.moderationCheckResult'),
  params: ModerationCheckResultSchema,
});
export type ShareModerationCheckResult = z.infer<typeof ShareModerationCheckResultSchema>;

// ── upload.delete ───────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §upload.delete params
// Shipped: crates/host/src/share/ws_messages.rs handle_delete() struct P
export const UploadDeleteParamsSchema = z.object({
  artifact_id: z.string(),
});
export type UploadDeleteParams = z.infer<typeof UploadDeleteParamsSchema>;

// Oracle: contracts/ws-explorer.md §upload.delete result: { deleted: true }
// Shipped: crates/host/src/share/ws_messages.rs handle_delete() — emits { deleted: true }
export const UploadDeleteResultSchema = z.object({
  id: z.string(),
  type: z.literal('upload.deleteResult'),
  params: z.object({
    deleted: z.literal(true),
  }),
});
export type UploadDeleteResult = z.infer<typeof UploadDeleteResultSchema>;

// ─────────────────────────────────────────────────────────────────────────────
// identity.* request params + response shapes
// ─────────────────────────────────────────────────────────────────────────────

// ── identity.show ───────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §identity.show params: {}
// Shipped: crates/host/src/share/ws_messages.rs handle_identity_show() — no params parsed
export const IdentityShowParamsSchema = z.object({});
export type IdentityShowParams = z.infer<typeof IdentityShowParamsSchema>;

// Oracle: contracts/ws-explorer.md §identity.show result
// Shipped: crates/host/src/share/ws_messages.rs handle_identity_show()
//
// fingerprint_emoji and fingerprint_words allow empty arrays — the shipped
// handler returns Vec::new() for both until sub-spec #006 follow-up lands.
// created_at is 0 until #006 (shipped handler hard-codes 0).
// backed_up drives the #015 first-publish gate. Always `false` until #006 wires
// real persistence of a successful identity.backup; UX treats false as "needs
// backup" and gates first publish accordingly (umbrella risk #10 accepted).
export const IdentityShowResponseSchema = z.object({
  id: z.string(),
  type: z.literal('identity.showResult'),
  params: z.object({
    pubkey_hex: z.string(),
    fingerprint_hex: z.string(),
    fingerprint_emoji: z.array(z.string()), // allows [] — #006 follow-up
    fingerprint_words: z.array(z.string()), // allows [] — #006 follow-up
    created_at: z.number().int(), // 0 until #006 follow-up
    backed_up: z.boolean(), // false until #006 persists backup events
  }),
});
export type IdentityShowResponse = z.infer<typeof IdentityShowResponseSchema>;

// ── identity.backup ─────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §identity.backup params
// Shipped: crates/host/src/share/ws_messages.rs handle_identity_backup() — NOT_IMPLEMENTED stub
export const IdentityBackupParamsSchema = z.object({
  passphrase: z.string(),
});
export type IdentityBackupParams = z.infer<typeof IdentityBackupParamsSchema>;

// Oracle: contracts/ws-explorer.md §identity.backup result: { encrypted_bytes_b64 }
// Shipped: crates/host/src/share/ws_messages.rs handle_identity_backup() — returns error envelope
//          (NOT_IMPLEMENTED until #006 follow-up; shape defined for future use)
export const IdentityBackupResultSchema = z.object({
  id: z.string(),
  type: z.literal('identity.backupResult'),
  params: z.object({
    encrypted_bytes_b64: z.string(),
  }),
});
export type IdentityBackupResult = z.infer<typeof IdentityBackupResultSchema>;

// ── identity.import ─────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §identity.import params
// Shipped: crates/host/src/share/ws_messages.rs handle_identity_import() — NOT_IMPLEMENTED stub
export const IdentityImportParamsSchema = z.object({
  encrypted_bytes_b64: z.string(),
  passphrase: z.string(),
  overwrite_existing: z.boolean(),
});
export type IdentityImportParams = z.infer<typeof IdentityImportParamsSchema>;

// Oracle: contracts/ws-explorer.md §identity.import result: { pubkey_hex, fingerprint_hex }
// Shipped: crates/host/src/share/ws_messages.rs handle_identity_import() — NOT_IMPLEMENTED
export const IdentityImportResultSchema = z.object({
  id: z.string(),
  type: z.literal('identity.importResult'),
  params: z.object({
    pubkey_hex: z.string(),
    fingerprint_hex: z.string(),
  }),
});
export type IdentityImportResult = z.infer<typeof IdentityImportResultSchema>;

// ── identity.rotate ─────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md — no rotate section in contract doc;
//         dispatch arm present in ws_messages.rs ("identity.rotate")
// Shipped: crates/host/src/share/ws_messages.rs handle_identity_rotate() — NOT_IMPLEMENTED stub
export const IdentityRotateParamsSchema = z.object({});
export type IdentityRotateParams = z.infer<typeof IdentityRotateParamsSchema>;

// Oracle: contracts/ws-explorer.md §identity.rotate
// Shipped: crates/host/src/share/ws_messages.rs handle_identity_rotate() → identity.rotateResult frame
//          (currently returns NOT_IMPLEMENTED error envelope; shape defined for future use)
export const IdentityRotateResultSchema = z.object({
  id: z.string(),
  type: z.literal('identity.rotateResult'),
  params: z.object({
    pubkey_hex: z.string(),
    fingerprint_hex: z.string(),
  }),
});
export type IdentityRotateResult = z.infer<typeof IdentityRotateResultSchema>;

// ─────────────────────────────────────────────────────────────────────────────
// report.* request params + response shapes
// ─────────────────────────────────────────────────────────────────────────────

// ── report.submit ───────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md §report.submit params (contract shows note as string)
// Shipped: crates/host/src/share/ws_messages.rs handle_report() struct P
//          handler parses note: String — required, NOT Option<String>
//
// DRIFT NOTE: Contract and shipped both treat note as required. UX treats it as
// optional by sending "" when empty. Do not mark .optional() — that would
// produce runtime BadInput if UI submits no note field.
export const ReportSubmitParamsSchema = z.object({
  artifact_id: z.string(),
  category: z.enum(['illegal', 'malware', 'impersonation', 'nsfw', 'other']),
  note: z.string(),
});
export type ReportSubmitParams = z.infer<typeof ReportSubmitParamsSchema>;

// Oracle: contracts/ws-explorer.md §report.submit result: { report_id, status: "received" }
// Shipped: crates/host/src/share/ws_messages.rs handle_report()
//          — emits { report_id: "", status: "received" }
//
// report_id allows empty string — shipped handler returns "" until #017 follow-up.
export const ReportSubmitResultSchema = z.object({
  id: z.string(),
  type: z.literal('report.submitResult'),
  params: z.object({
    report_id: z.string(), // allows "" — #017 follow-up
    status: z.literal('received'),
  }),
});
export type ReportSubmitResult = z.infer<typeof ReportSubmitResultSchema>;

// ─────────────────────────────────────────────────────────────────────────────
// config.* request params + response shapes
// ─────────────────────────────────────────────────────────────────────────────

// ── config.vocab ────────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md — config.vocab not listed in message index
//         but dispatch arm is present in ws_messages.rs
// Shipped: crates/host/src/share/ws_messages.rs handle_config_vocab()
//          crates/host/src/share/client.rs VocabDoc
export const ConfigVocabParamsSchema = z.object({});
export type ConfigVocabParams = z.infer<typeof ConfigVocabParamsSchema>;

// Oracle: contracts/ws-explorer.md — config.vocab not listed in message index but dispatch arm is present
// Shipped: crates/host/src/share/ws_messages.rs handle_config_vocab() → config.vocabResult frame
export const ConfigVocabResultSchema = z.object({
  id: z.string(),
  type: z.literal('config.vocabResult'),
  params: z.object({
    tags: z.array(z.string()),
    version: z.number().int().nonnegative(),
  }),
});
export type ConfigVocabResult = z.infer<typeof ConfigVocabResultSchema>;

// ── config.limits ────────────────────────────────────────────────────────────

// Oracle: contracts/ws-explorer.md — config.limits not in message index
//         but dispatch arm is present in ws_messages.rs
// Shipped: crates/host/src/share/ws_messages.rs handle_config_limits()
//          crates/host/src/share/client.rs config_limits() → BundleLimits
export const ConfigLimitsParamsSchema = z.object({});
export type ConfigLimitsParams = z.infer<typeof ConfigLimitsParamsSchema>;

// Oracle: contracts/ws-explorer.md — config.limits not in message index but dispatch arm is present
// Shipped: crates/host/src/share/ws_messages.rs handle_config_limits() → config.limitsResult frame
export const ConfigLimitsResultSchema = z.object({
  id: z.string(),
  type: z.literal('config.limitsResult'),
  params: z.object({
    max_bundle_compressed: z.number().int().nonnegative(),
    max_bundle_uncompressed: z.number().int().nonnegative(),
    max_entries: z.number().int().nonnegative(),
    version: z.number().int(),
    updated_at: z.number().int(),
  }),
});
export type ConfigLimitsResult = z.infer<typeof ConfigLimitsResultSchema>;

// ─────────────────────────────────────────────────────────────────────────────
// workspace.* request params + response shapes
// (upload-flow-redesign Wave A0 — spec §8.8 + INV-7.1.10)
// ─────────────────────────────────────────────────────────────────────────────

// Oracle: docs/superpowers/specs/2026-04-21-upload-flow-redesign-design.md §8.1
// Shipped: crates/host/src/share/sidecar.rs PublishSidecar
// Generated TS oracle: packages/shared-types/src/generated/PublishSidecar.ts
//
// On-disk shape of `.omni-publish.json`. The renderer's Step 1 source picker
// reads `sidecar?.author_pubkey_hex` to decide whether to render the
// "linked-artifact" banner (INV-7.1.13) and pivot Step 4 into update mode
// (INV-7.5.*). All four fields are required in the on-disk shape.
export const PublishSidecarSchema = z.object({
  artifact_id: z.string(),
  author_pubkey_hex: z.string(),
  version: z.string(),
  last_published_at: z.string(),
});
export type PublishSidecar = z.infer<typeof PublishSidecarSchema>;

// Oracle: spec §8.8 + INV-7.1.10
// Shipped: crates/host/src/share/ws_messages.rs PublishablesEntry
// Generated TS oracle: packages/shared-types/src/generated/PublishablesEntry.ts
//
// One row in the `workspace.listPublishables` response. The renderer's
// SourceStep renders these directly (no client-side derivation): widget count
// + modified date drive INV-7.1.10's metadata subtitle, `has_preview` gates
// the placeholder vs `.omni-preview.png` thumbnail (INV-7.1.9), and
// `sidecar` drives both the linked-artifact banner (INV-7.1.13) and the
// pubkey-mismatch warning (INV-7.6.4).
export const PublishablesEntrySchema = z.object({
  kind: z.string(),
  workspace_path: z.string(),
  name: z.string(),
  widget_count: z.number().int().nonnegative().nullable(),
  modified_at: z.string(),
  has_preview: z.boolean(),
  sidecar: PublishSidecarSchema.nullable(),
});
export type PublishablesEntry = z.infer<typeof PublishablesEntrySchema>;

// Oracle: spec §8.8 + INV-7.1.10
// Shipped: crates/host/src/share/ws_messages.rs handle_list_publishables() struct P
//
// Optional `kind` filter narrows the response to overlays-only or themes-only.
// Omit / null returns both. Unknown values fall through to "no filter" on the
// host (defensive — see the handler doc comment).
export const WorkspaceListPublishablesParamsSchema = z.object({
  kind: z.enum(['overlay', 'theme']).optional(),
});
export type WorkspaceListPublishablesParams = z.infer<typeof WorkspaceListPublishablesParamsSchema>;

// Oracle: spec §8.8 + INV-7.1.10
// Shipped: crates/host/src/share/ws_messages.rs handle_list_publishables() —
//          emits { id, type: 'workspace.listPublishablesResult', params: { entries } }
export const WorkspaceListPublishablesResultSchema = z.object({
  id: z.string(),
  type: z.literal('workspace.listPublishablesResult'),
  params: z.object({
    entries: z.array(PublishablesEntrySchema),
  }),
});
export type WorkspaceListPublishablesResult = z.infer<typeof WorkspaceListPublishablesResultSchema>;

// ─────────────────────────────────────────────────────────────────────────────
// Type-level request + subscription registries
// ─────────────────────────────────────────────────────────────────────────────

/**
 * ShareRequestMap — type-level registry mapping message type strings to their
 * params and result shapes. Enables generic send() patterns like:
 *
 *   function send<T extends keyof ShareRequestMap>(
 *     type: T,
 *     params: ShareRequestMap[T]["params"]
 *   ): Promise<ShareRequestMap[T]["result"]>
 */
export interface ShareRequestMap {
  'explorer.list': {
    params: ExplorerListParams;
    result: ExplorerListResult;
  };
  'explorer.get': {
    params: ExplorerGetParams;
    result: ExplorerGetResult;
  };
  'explorer.install': {
    params: ExplorerInstallParams;
    result: ExplorerInstallResult;
  };
  'explorer.preview': {
    params: ExplorerPreviewParams;
    result: ExplorerPreviewResult;
  };
  'explorer.cancelPreview': {
    params: ExplorerCancelPreviewParams;
    result: ExplorerCancelPreviewResult;
  };
  'explorer.fork': {
    params: ExplorerForkParams;
    result: ExplorerForkResult;
  };
  'upload.pack': {
    params: UploadPackParams;
    result: UploadPackResult;
  };
  'upload.publish': {
    params: UploadPublishParams;
    result: UploadPublishResult;
  };
  'upload.update': {
    params: UploadUpdateParams;
    result: UploadUpdateResult;
  };
  'upload.delete': {
    params: UploadDeleteParams;
    result: UploadDeleteResult;
  };
  'identity.show': {
    params: IdentityShowParams;
    result: IdentityShowResponse;
  };
  'identity.backup': {
    params: IdentityBackupParams;
    result: IdentityBackupResult;
  };
  'identity.import': {
    params: IdentityImportParams;
    result: IdentityImportResult;
  };
  'identity.rotate': {
    params: IdentityRotateParams;
    result: IdentityRotateResult;
  };
  'report.submit': {
    params: ReportSubmitParams;
    result: ReportSubmitResult;
  };
  'config.vocab': {
    params: ConfigVocabParams;
    result: ConfigVocabResult;
  };
  'config.limits': {
    params: ConfigLimitsParams;
    result: ConfigLimitsResult;
  };
  'workspace.listPublishables': {
    params: WorkspaceListPublishablesParams;
    result: WorkspaceListPublishablesResult;
  };
  // Wave B1 — Step 2 Preview Image moderation gate (INV-7.7.2 site #1).
  'share.moderationCheck': {
    params: ShareModerationCheckParams;
    result: ShareModerationCheckResult;
  };
}

/**
 * ShareSubscriptionMap — type-level registry for streaming event frame shapes.
 * Maps subscription event type strings to their frame shape for each streaming
 * event emitted by the host before the terminal *Result frame.
 */
// NOTE: Update flows share the 'upload.publishProgress' progress type —
// pump_to_ws() uses it for both publish and update. No separate
// 'upload.updateProgress' exists (verified: progress.rs:81).
export interface ShareSubscriptionMap {
  'explorer.installProgress': ExplorerInstallProgress;
  'upload.publishProgress': UploadPublishProgress;
  'upload.packProgress': UploadPackProgress;
}

// ─────────────────────────────────────────────────────────────────────────────
// Runtime Zod schema registries
// ─────────────────────────────────────────────────────────────────────────────

/**
 * ShareResponseSchemas — runtime registry mapping result-frame type strings
 * to their Zod schemas. Used for runtime validation of WS messages received
 * from the host.
 */
export const ShareResponseSchemas = {
  'explorer.listResult': ExplorerListResultSchema,
  'explorer.getResult': ExplorerGetResultSchema,
  'explorer.installResult': ExplorerInstallResultSchema,
  'explorer.previewResult': ExplorerPreviewResultSchema,
  'explorer.cancelPreviewResult': ExplorerCancelPreviewResultSchema,
  'explorer.forkResult': ExplorerForkResultSchema,
  'upload.packResult': UploadPackResultSchema,
  'upload.publishResult': UploadPublishResultSchema,
  'upload.updateResult': UploadUpdateResultSchema,
  'upload.deleteResult': UploadDeleteResultSchema,
  'identity.showResult': IdentityShowResponseSchema,
  'identity.backupResult': IdentityBackupResultSchema,
  'identity.importResult': IdentityImportResultSchema,
  'identity.rotateResult': IdentityRotateResultSchema,
  'report.submitResult': ReportSubmitResultSchema,
  'config.vocabResult': ConfigVocabResultSchema,
  'config.limitsResult': ConfigLimitsResultSchema,
  'workspace.listPublishablesResult': WorkspaceListPublishablesResultSchema,
  'share.moderationCheckResult': ShareModerationCheckResultSchema,
} as const satisfies Record<string, z.ZodTypeAny>;

/** Exhaustive union of every WS response/progress type string a consumer may receive. Useful for discriminated switch statements. */
export type ShareResponseType = keyof typeof ShareResponseSchemas;

/**
 * ShareSubscriptionSchemas — runtime registry mapping streaming-event type
 * strings to their Zod schemas. Used for runtime validation of progress frames.
 */
// NOTE: Update flows share the 'upload.publishProgress' progress type —
// pump_to_ws() uses it for both publish and update. No separate
// 'upload.updateProgress' exists (verified: progress.rs:81).
export const ShareSubscriptionSchemas = {
  'explorer.installProgress': ExplorerInstallProgressSchema,
  'upload.publishProgress': UploadPublishProgressSchema,
  'upload.packProgress': UploadPackProgressSchema,
} as const satisfies Record<string, z.ZodTypeAny>;

/** Exhaustive union of every subscription (streaming) type string. Used by `useShareWs.subscribe<T>()` inference. */
export type ShareSubscriptionType = keyof typeof ShareSubscriptionSchemas;
