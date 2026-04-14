# Contributing to omni-host

## Build modes

`omni-host` builds in two modes:

### 1. Default (contributor) mode

```bash
cargo build --package omni-host
```

Uses `StubGuard` from `omni-guard-trait` for abuse prevention and runs
against a development Worker endpoint with relaxed limits. This mode does
NOT require any access to the private `omni-guard` repository; the
dependency is optional and gated behind the `guard` Cargo feature.

All contribution PRs must build and pass tests in default mode.

### 2. Release mode

```bash
cargo build --release --package omni-host --features guard,strict-integrity
```

Pulls the private `omni-guard` crate from
`ssh://git@github.com/djowinz/omni-guard.git` via Cargo's git dependency
mechanism. Only maintainers with SSH access to that repo can build this
mode locally; release CI supplies the deploy key. Release-mode changes
are verified in CI, not in contributor-facing PRs.

Note the `strict-integrity` feature: it flips the integrity-hash constant
from `option_env!` (silent skip when unset) to `env!` (compile error when
unset). Release CI always builds with both features; contributors building
release mode locally who don't also set `OMNI_GUARD_TEXT_SHA256` should
omit `strict-integrity`.

## Browsing the private `omni-guard` source

Maintainers with SSH access may clone the private repo wherever is
convenient:

```bash
git clone git@github.com:djowinz/omni-guard.git ../omni-guard
```

It is not required for any build and is not part of this repo. Cargo pulls
it independently via the git URL declared in `crates/host/Cargo.toml`.
