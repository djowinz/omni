// @omni/shared-types — TypeScript views of Rust types defined across the
// Omni Rust workspace (crates/shared and crates/host).
//
// Files under ./generated are produced by `cargo test` via ts-rs. Do not
// edit ./generated directly — they are overwritten.
//
// Consumers import from "@omni/shared-types", never from "./generated/..."
// directly. The barrel below curates the public entry point.

// --- Sensor data (crates/shared) ---
export type { SensorSnapshot } from './generated/SensorSnapshot';
export type { CpuData } from './generated/CpuData';
export type { GpuData } from './generated/GpuData';
export type { RamData } from './generated/RamData';
export type { FrameData } from './generated/FrameData';

// --- Application configuration (crates/host) ---
export type { Config } from './generated/Config';
export type { KeybindConfig } from './generated/KeybindConfig';

// --- Omni file parser output (crates/host) ---
export type { OmniFile } from './generated/OmniFile';
export type { Widget } from './generated/Widget';
export type { HtmlNode } from './generated/HtmlNode';
export type { ConditionalClass } from './generated/ConditionalClass';
export type { DpiScale } from './generated/DpiScale';

// --- Parse diagnostics (crates/host) ---
export type { ParseError } from './generated/ParseError';
export type { Severity } from './generated/Severity';

// --- Share-hub wire types (crates/host) ---
export type { CachedArtifactDetail } from './generated/CachedArtifactDetail';
export type { UploadResult } from './generated/UploadResult';
export type { UploadStatus } from './generated/UploadStatus';

// --- Upload-flow-redesign Wave A0 wire contracts (crates/host) ---
// `upload.packProgress` per-stage stream payload (spec §8.8) + the
// `workspace.listPublishables` per-row shape (INV-7.1.10). Authored alongside
// the renderer Zod schemas in `apps/desktop/renderer/lib/share-types.ts`;
// the sidecar types-test there enforces bidirectional assignability.
export type { PackProgress } from './generated/PackProgress';
export type { PackStage } from './generated/PackStage';
export type { StageStatus } from './generated/StageStatus';
export type { PublishablesEntry } from './generated/PublishablesEntry';
export type { PublishSidecar } from './generated/PublishSidecar';

// --- Upload-flow-redesign Wave B1 wire contracts (crates/host) ---
// `share.moderationCheck` result payload (INV-7.7.2 site #1). Authored alongside
// the renderer Zod schema in `apps/desktop/renderer/lib/share-types.ts`; the
// sidecar types-test there enforces bidirectional assignability.
export type { ModerationCheckResult } from './generated/ModerationCheckResult';
