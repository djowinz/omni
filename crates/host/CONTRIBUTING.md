# Contributing to omni-host

`omni-host` is the core service binary in `apps/desktop/`. It depends on
`omni-guard` (`crates/omni-guard/`), which provides the device-fingerprint,
anti-debug, integrity, and VM-detection traits used to gate abuse-prevention
behaviour.

Both crates are public and build with a plain:

```bash
cargo build --package host
```

No SSH keys, no private repos, no `--features` flag required.

## Debugging the host

The host's anti-debug poisons the `device_id()` value when a debugger is
attached, which can interfere with debugging sessions. To bypass for
local development, build with the `dev-no-guard` feature:

```bash
cargo run -p host --features dev-no-guard -- --service
```

This swaps `RealGuard` for `DisabledGuard` (no-op everywhere). **Release
builds MUST NOT enable this feature** — the release startup check exits
non-zero if `guard.enforcement_mode()` returns `Disabled`.
