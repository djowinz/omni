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

// --- Parse diagnostics (crates/host) ---
export type { ParseError } from './generated/ParseError';
export type { Severity } from './generated/Severity';
