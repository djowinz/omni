# Repository Structure

This document defines the repository layout and the rules for where new code
goes. It exists so contributors and AI agents can maintain structural
consistency as the project evolves. When in doubt, **update this document
first, then the code**.

**Companion docs:** [`docs/architecture.md`](docs/architecture.md) describes
how the system runs (processes, IPC, data flow). This document describes how
the repo is laid out and why.

## Top-level layout

```
omni/
├── apps/          Release targets (independently-deployed products)
├── crates/        Rust workspace (libraries + binaries shipped within apps)
├── packages/      TypeScript workspace (shared Node packages)
├── tools/         Dev-only CLIs (never shipped to end users)
├── stubs/         Rust fallback crates for private-repo dependencies
├── vendor/        Third-party prebuilt binaries (DLLs, headers)
├── docs/          Public documentation (in VCS)
│   ├── architecture.md   Runtime architecture
│   ├── contracts/        Cross-component contracts (schemas, APIs, algorithms)
│   ├── contributing.md   Contribution guide
│   └── superpowers/      GITIGNORED — personal AI-tool artifacts, NEVER in VCS
├── scripts/       Shell orchestration (release, installer, deploy)
├── .github/       CI/CD workflows
├── Cargo.toml     Rust workspace root
├── package.json   pnpm workspace root
└── STRUCTURE.md   This document
```

## The four categories

These four directories cover all first-class code in the repo. Every new
thing belongs in exactly one of them.

### `apps/` — release targets

An `apps/*` directory is an **independently-versioned, independently-deployed
product**. Each app has its own release pipeline and its own user-visible
distribution channel.

| Current | Type | Distribution |
|---|---|---|
| `apps/desktop/` | Electron + Nextron | NSIS installer for Windows |
| `apps/worker/` | Cloudflare Worker | `wrangler deploy` |

**Put something here if:** it's the top-level artifact a user or operator
deploys/installs, with its own release cadence.

**Do not put something here if:** it's a subprocess of another app (→ `crates/`),
a library (→ `crates/` or `packages/`), or used only by maintainers (→ `tools/`).

### `crates/` — Rust workspace

All Rust code. Both libraries and binaries that are **bundled into an app at
build time**. Rust binaries here (currently `host`, `overlay`) are components
of `apps/desktop/` — they ship as subprocesses inside the desktop installer
and have no independent release. Their version is the desktop app's version.

| Current | Type | Role |
|---|---|---|
| `crates/host/` | bin (`omni-host.exe`) | Core service: WMI, IPC, bundle lifecycle |
| `crates/overlay/` | bin (`omni-overlay.exe`) | Anti-cheat fallback overlay renderer |
| `crates/shared/` | lib | Types shared across Rust + TS (ts-rs source of truth) |
| `crates/bundle/` | lib | Bundle validation |
| `crates/sanitize/` | lib | CSS/HTML sanitizer |
| `crates/identity/` | lib | Ed25519 keypair + JWS signing |
| `crates/omni-guard-trait/` | lib | Trait for the private `omni-guard` crate |
| `crates/ultralight-sys/` | lib (FFI) | Ultralight SDK bindings |

**Put something here if:** it's Rust code that `apps/desktop/` depends on, or
a Rust library consumed by other Rust code.

**Do not put something here if:** it's a maintainer-only CLI (→ `tools/`),
TypeScript (→ `packages/` or `apps/`), or an end-user-facing product with its
own release channel (→ `apps/`, though no such Rust app exists today).

### `packages/` — TypeScript workspace

Shared TypeScript packages consumed by `apps/*`. Each package has a
`@omni/*` NPM scope name and is a pnpm workspace member.

| Current | NPM name | Role |
|---|---|---|
| `packages/shared-types/` | `@omni/shared-types` | ts-rs output from `crates/shared`; the TS view of Rust types |

**Put something here if:** it's TypeScript consumed by two or more apps, or
by one app and a future-expected sibling.

**Do not put something here if:** it's used by only one app and has no
realistic cross-app reuse (keep it inside the app).

### `tools/` — dev-only CLIs

Command-line tools for maintainers, CI, or contributors. **Never shipped
to end users.**

| Current | Language | Role |
|---|---|---|
| `tools/admin/` | Rust | Moderation CLI for the themes Worker |
| `tools/integrity-hash/` | Rust | SHA-256 utility used in the release pipeline |

**Put something here if:** it's a CLI that only maintainers or CI will run,
regardless of language.

**Do not put something here if:** it ends up on an end user's machine (→ `apps/`
or `crates/`).

## Naming conventions

### Package names — drop the `omni-` prefix

The directory path (`omni/crates/...`, `omni/packages/...`) already provides
the namespace. Package names themselves are short and role-descriptive.

Good: `host`, `overlay`, `bundle`, `sanitize`, `identity`, `shared`
Bad: `omni-host`, `omni-bundle`

**Exceptions (all constraint-driven):**

- **`ultralight-sys`** — Rust FFI crates must end in `-sys`. Non-negotiable.
- **`omni-guard`** and **`omni-guard-trait`** — the name of the private
  `omni-guard` crate must match its remote git repo for `[patch.'ssh://...']`
  to resolve. `omni-guard-trait` keeps the prefix for visual alignment with
  its pair.

### Binary names — keep `omni-` prefix for end-user binaries

Binaries that end up on users' machines keep the `omni-` prefix so they're
identifiable in Task Manager, Windows process lists, crash reports, and
installer file listings.

- `crates/host` (package: `host`) → binary: **`omni-host.exe`**
- `crates/overlay` (package: `overlay`) → binary: **`omni-overlay.exe`**

This split is expressed in `Cargo.toml`:

```toml
[package]
name = "host"

[[bin]]
name = "omni-host"
path = "src/main.rs"
```

**Dev tools** (`tools/*`) do not carry the prefix — they're only run by
maintainers who know the repo context.

- `tools/admin` → binary: `admin`
- `tools/integrity-hash` → binary: `integrity-hash`

## Cross-cutting concerns

### Shared types (`crates/shared` ↔ `packages/shared-types`)

`crates/shared` is the **single source of truth** for types shared between
Rust and TypeScript. Types are declared as Rust structs with `#[derive(TS)]`
and emitted as TypeScript via `ts-rs` into `packages/shared-types/src/generated/`.

**Workflow:**
1. Add/change a type in `crates/shared/src/*.rs`
2. Run `cargo test -p shared` (ts-rs generates the TS on the test run)
3. Commit both the Rust change and the regenerated `.ts` files in the same PR

**CI enforces drift:** if committed TS doesn't match generator output,
the PR fails.

**Consumers** import from `@omni/shared-types`, never from the generated path
directly:

```ts
import type { SensorSnapshot } from "@omni/shared-types";
```

### Shared config

All shared configuration lives at the repo root. Member packages extend.

| Root file | Purpose |
|---|---|
| `tsconfig.base.json` | Shared TS compiler options |
| `.prettierrc`, `.prettierignore` | Formatting (all Node packages) |
| `eslint.config.js` | Flat-config lint rules (TS + React + Node + Worker presets) |
| `package.json` | pnpm workspace root; scripts delegate via `pnpm -r` |
| `pnpm-workspace.yaml` | Declares `apps/*` and `packages/*` as members |
| `pnpm-lock.yaml` | Single consolidated lockfile |

Member packages should **not** redeclare configs — extend from root. If a
member genuinely needs to override, do so with the narrowest possible delta
in that member's config file.

### `stubs/`

Fallback Rust crates that satisfy private-repo dependencies during
open-source builds. The root `Cargo.toml` patches the private git URL to
the local stub path. CI's release pipeline overrides the patch with
`--config` to pull the real private crate.

Only add a new stub here when introducing a new private-crate dependency.
See `crates/host/CONTRIBUTING.md` for the full sync workflow.

### `vendor/`

Third-party binaries, DLLs, and headers shipped as-is — not built from source
via Cargo/npm. Each subdirectory is one vendored dependency.

Currently: `vendor/ultralight/` (Ultralight SDK, consumed by
`crates/ultralight-sys` at build time and copied into the desktop installer).

### `docs/` — what does and does not belong in VCS

**In VCS:**
- `docs/architecture.md` — runtime architecture
- `docs/contributing.md` — contribution guide
- `docs/contracts/` — cross-component authoritative contracts (schemas,
  algorithms, API definitions). Files here are consumed at build/test time
  by production code, so they must be in VCS. See `docs/contracts/README.md`.
- `docs/images/` — screenshots, logos, banners
- Per-crate/per-app `README.md` and `CONTRIBUTING.md` where useful

**Never in VCS:**
- `docs/superpowers/` is **gitignored** and must stay that way. It contains
  personal AI-tool artifacts (specs, plans, retros). These are not
  project documentation and must never be committed.

If a design decision is worth preserving in VCS, promote it to a sanctioned
doc (inline in `docs/architecture.md`, a new `docs/<topic>.md`, or for
cross-component boundaries, `docs/contracts/`) — not by moving superpowers
content into VCS.

## Decision tree: where does this new thing go?

```
New code
│
├─ Is it a CLI only maintainers/CI run?
│    └─ YES → tools/<name>/
│
├─ Does it end up on an end user's machine or deploy to prod?
│    │
│    ├─ YES, with its own release pipeline
│    │    └─ apps/<name>/
│    │
│    └─ YES, but bundled inside another app (subprocess, DLL, etc.)
│         └─ crates/<name>/ (if Rust) — binary, with [[bin]] prefix-override
│
├─ Is it a library consumed by other code in the repo?
│    │
│    ├─ Rust → crates/<name>/
│    └─ TypeScript, shared across ≥2 apps → packages/<name>/
│    └─ TypeScript, used by only one app → inside that app
│
└─ Is it a fallback for a private crate? → stubs/<name>/
   Is it a third-party prebuilt binary? → vendor/<name>/
   Is it a cross-component contract (schema/API/algorithm)? → docs/contracts/
```

## Evolving this document

This file is the source of truth for structural conventions. Two rules:

1. **Adding a new top-level category, or changing a naming convention, requires
   updating this file in the same PR.** Reviewers enforce this.

2. **Adding a new member within an existing category** (a new crate, a new
   package, a new tool) does not require updating this file — the existing
   conventions apply. Update the examples table only if the addition is
   load-bearing enough that future readers would benefit from seeing it.
