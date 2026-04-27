// apps/desktop/renderer/lib/omni-file.types-test.ts
//
// Type-level assertion that the editor's view of OmniFile.dpi_scale matches
// the ts-rs-generated wire shape from crates/host/src/omni/types.rs. Catches
// silent wire drift between Rust and editor consumers per writing-lessons §A8.
//
// Convention: import generated types from @omni/shared-types, never from
// generated/* directly. See packages/shared-types/src/index.ts for the barrel.

import type { DpiScale, OmniFile } from '@omni/shared-types';

// ---- DpiScale shape -------------------------------------------------------

// Forward: every variant the Rust enum can produce is one of the two
// expected discriminated-union members. If ts-rs ever changes the encoding
// (e.g. drops the `kind` discriminant or renames it), this fails to compile.
type _DpiScaleAuto = { kind: 'auto' } extends DpiScale ? true : never;
type _DpiScaleManual = { kind: 'manual'; value: number } extends DpiScale ? true : never;

const _dpiScaleAuto: _DpiScaleAuto = true;
const _dpiScaleManual: _DpiScaleManual = true;

// ---- OmniFile.dpi_scale shape ---------------------------------------------

// Confirm OmniFile carries the optional dpi_scale field with the expected
// type. ts-rs emits Rust `Option<T>` as `T | null`.
type _OmniFileHasDpiScale = OmniFile extends { dpi_scale: DpiScale | null } ? true : never;

const _omniFileHasDpiScale: _OmniFileHasDpiScale = true;

// Silence "unused" warnings — declarations exist purely for type-level
// side effects.
export const __typeTestSentinels = {
  _dpiScaleAuto,
  _dpiScaleManual,
  _omniFileHasDpiScale,
};
