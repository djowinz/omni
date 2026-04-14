# Contributing to omni-host

## Build modes

`omni-host` depends on `omni-guard`, which is partly public (the `Guard`
trait + `StubGuard` live in `crates/omni-guard-trait`) and partly private
(the real `RealGuard` implementation lives in
`git@github.com:djowinz/omni-guard.git`, accessible only to maintainers).

The public repo ships a committed stub at `stubs/omni-guard/` that
satisfies the workspace resolver by default. A workspace-level `[patch]`
entry in the root `Cargo.toml` redirects cargo's resolution of the
private git URL to this stub, so contributors without SSH access never
need to clone the private repo.

### 1. Default (contributor) mode

```bash
cargo build --package omni-host
```

Uses `StubGuard` from `omni-guard-trait` for abuse prevention and runs
against a development Worker endpoint with relaxed limits. The `omni-guard`
Cargo feature is off; no reference to the private crate is compiled.

```bash
cargo build --package omni-host --features guard
```

Still works without SSH access: the `[patch]` in the root `Cargo.toml`
resolves `omni-guard` to `stubs/omni-guard/`, whose `RealGuard::new()`
returns no-op stub behavior (same as `StubGuard`). Useful for testing the
factory-mode swap locally.

All contribution PRs must build and pass tests in default mode.

### 2. Release mode

Release-mode builds happen in CI only. The workflow in
`.github/workflows/release.yml` overrides the `[patch]` via `cargo --config`
to point at the real private crate:

```bash
cargo build --release --package omni-host --features guard \
  --config 'patch."ssh://git@github.com/djowinz/omni-guard.git".omni-guard={ git = "ssh://git@github.com/djowinz/omni-guard.git", branch = "main", features = ["strict-integrity"] }'
```

Maintainers with SSH access can reproduce this locally; others cannot.

The `strict-integrity` feature on `omni-guard` swaps `option_env!` for
`env!` on the integrity-hash constant, forcing a compile error if
`OMNI_GUARD_TEXT_SHA256` is unset. Release CI always sets both.

### Keeping the stub in sync

`stubs/omni-guard/src/lib.rs` exposes the subset of the private crate's
public API that `omni-host` actually calls (`RealGuard::new()` + the
`Guard` trait impl). If a change to `omni-host` reaches a new symbol on
`omni_guard::RealGuard`, update `stubs/omni-guard/` in the same commit —
otherwise default builds break.

The private repo's CI compiles the stub against the real crate's public
API to catch drift before it reaches this repo.

## Browsing the private `omni-guard` source

Maintainers with SSH access may clone the private repo wherever is
convenient:

```bash
git clone git@github.com:djowinz/omni-guard.git ../omni-guard
```

It is not required for any build and is not part of this repo.
