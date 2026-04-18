# Contracts

Authoritative interface definitions shared across the repo. Files in this
directory are consumed at build/test time by production code (schemas
validated against, algorithms implemented from, APIs documented against),
so they are **tracked in VCS** — unlike the gitignored `docs/superpowers/`
which holds personal AI-tool artifacts (design specs, plans, retros).

## Current contracts

| File                          | Owner                          | Consumed by                                                                                |
| ----------------------------- | ------------------------------ | ------------------------------------------------------------------------------------------ |
| `bundle-manifest.schema.json` | `crates/bundle`                | `crates/bundle/tests/integration_schema.rs` validates packed manifests against this schema |
| `canonical-hash-algorithm.md` | `crates/bundle`                | `crates/bundle/src/hash.rs` implements this algorithm                                      |
| `data-sensor-attributes.md`   | `crates/host` + `apps/desktop` | Host emits sensor paths; renderer consumes them                                            |
| `identity-file-format.md`     | `crates/identity`              | Load/save format for Ed25519 keypairs                                                      |
| `worker-api.md`               | `apps/worker`                  | HTTP + error contract between desktop and Worker                                           |
| `ws-explorer.md`              | `apps/desktop` ↔ `crates/host` | WebSocket explorer protocol                                                                |

## Stability and versioning

Contracts define cross-component boundaries. Change with intention:

- **Breaking changes** require coordinated updates across every consumer listed above.
- **Additive changes** (new optional fields, new error codes) are preferred.
- **Renames or removals** should deprecate first when feasible.

When a schema or algorithm changes, update the consuming code in the same PR.

## See also

- [`../../STRUCTURE.md`](../../STRUCTURE.md) — how `docs/contracts/` fits the repo layout
- [`../architecture.md`](../architecture.md) — runtime architecture (companion doc)
