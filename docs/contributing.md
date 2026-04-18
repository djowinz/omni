# Contributing

## Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- [Node.js](https://nodejs.org/) (v20+)
- [pnpm](https://pnpm.io/) (v10+)
- [GitHub CLI](https://cli.github.com/) (`gh`) — for releases
- Windows 10/11 (64-bit) — required for Win32 APIs

## Getting Started

```bash
git clone https://github.com/djowinz/omni.git
cd omni
make install    # Install Node.js dependencies
make build      # Build everything + run tests
make dev        # Start the desktop editor in dev mode
```

The host service needs to be running for the editor to connect. Build it separately if needed:

```bash
make rust
cargo run -p omni-host -- --service
```

## Development Workflow

1. Create a feature branch from `main`
2. Make changes and write tests
3. Run `make test` to validate
4. Open a PR to `main` — CI runs automatically
5. On merge, the release workflow bumps the version and publishes

## Commit Conventions

Commit messages determine the version bump when merged to `main`:

| Prefix          | Bump                  | Example                               |
| --------------- | --------------------- | ------------------------------------- |
| `major:`        | Major (1.0.0 → 2.0.0) | `major: overhaul IPC protocol`        |
| `feat:`         | Minor (1.1.0 → 1.2.0) | `feat: add temperature graph widget`  |
| Everything else | Patch (1.1.1 → 1.1.2) | `fix: correct frame time calculation` |

The highest-level prefix in the commit log wins. If any commit says `major:`, the release is a major bump regardless of other commits.

Commits with `[skip ci]` in the message are excluded from release notes and do not trigger the release workflow.

## Project Structure

See [Architecture](architecture.md) for a full breakdown. The short version:

- `apps/desktop/` — Electron + Next.js editor
- `crates/` — Rust workspace (host, overlay, shared, ultralight-sys)
- `scripts/` — Composable build scripts
- `Makefile` — Unified entry point

## Testing

### Rust

```bash
make test-rust          # or: cargo test --workspace
```

Tests cover: .omni parsing, sensor path validation, edit distance suggestions, IPC protocol, and condition evaluation.

`cargo test` also regenerates TypeScript bindings in `apps/desktop/renderer/generated/` via ts-rs.

### Desktop

```bash
make test-desktop       # or: cd apps/desktop && pnpm test
```

Tests cover: widget parsing, metric formatting, sensor mapping, state reducer (overlay CRUD, tab management), and class binding encoding.

Tests follow BDD patterns and focus on function input/output, not framework rendering.

## Build Scripts

All scripts are in `scripts/` and composed via the Makefile:

| Script                 | Purpose                                                         |
| ---------------------- | --------------------------------------------------------------- |
| `build-rust.sh`        | Build Rust binaries + run tests (`--skip-tests` flag)           |
| `build-desktop.sh`     | Install deps + run tests + build Electron (`--skip-tests` flag) |
| `build-installer.sh`   | Verify artifacts + package NSIS installer                       |
| `gen-release-notes.sh` | Generate markdown from git log since last tag                   |
| `release.sh`           | Full pipeline: bump → tag → build → test → package → publish    |

## Linting

```bash
make lint               # Run clippy + eslint
```

## Releasing

Releases happen automatically on merge to `main`. To release manually:

```bash
make release INCREMENT=patch    # or: minor, major
```

Or interactively:

```bash
./scripts/release.sh
```
