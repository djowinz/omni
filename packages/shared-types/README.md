# @omni/shared-types

TypeScript views of Rust types used across the Omni monorepo.

## Source of truth

This package is **generated** from the Rust crate [`crates/shared`](../../crates/shared)
via [`ts-rs`](https://github.com/Aleph-Alpha/ts-rs). Do **not** edit files
under `src/generated/` directly — they will be overwritten.

## Workflow

1. Change or add a type in `crates/shared/src/*.rs` with `#[derive(TS)]`.
2. Run `cargo test -p shared` from the repo root.
   ts-rs emits `.ts` files into `src/generated/` as a side effect of the test run.
3. Update `src/index.ts` to re-export any new types with appropriate grouping.
4. Commit both the Rust change and the regenerated TS in the same PR.

CI enforces drift: if committed `src/generated/` does not match generator
output, the PR fails.

## Consumer usage

```ts
import type { SensorSnapshot, BitmapHeader } from "@omni/shared-types";
```

Never import from `@omni/shared-types/src/generated/...` directly — the
barrel in `src/index.ts` is the public entry point.

## See also

- [`../../STRUCTURE.md`](../../STRUCTURE.md) — how this package fits the repo
- [`../../crates/shared/`](../../crates/shared) — the Rust source of truth
