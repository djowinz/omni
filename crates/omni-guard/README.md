# omni-guard

Abuse-prevention primitives for [Omni](https://github.com/djowinz/omni). Probe-only — no signing or key handling (signing lives in `crates/identity`).

## Primitives

- **Device fingerprint** — `Guard::device_id()` returns a SHA-256 over MAC + Windows MachineGuid + CPU brand. Used by the worker for per-device rate-limit buckets so an attacker can't trivially defeat per-pubkey limits by minting fresh identities.
- **Self-integrity** — `Guard::verify_self_integrity()` hashes the running PE `.text` section and compares against a build-time-baked `OMNI_GUARD_TEXT_SHA256` env constant. Catches binary patching post-launch.
- **Anti-debug** — five layered checks (`IsDebuggerPresent`, PEB.BeingDebugged direct read, `NtQueryInformationProcess(ProcessDebugPort)`, hardware breakpoint registers, RDTSC timing). `#[inline(never)]` keeps each check observable so the **silent-poison** pattern in `device.rs` can XOR the device_id when any fires.
- **VM detection** — `Guard::is_vm()` via CPUID hypervisor bit + vendor string match. Worker uses the result to apply 25% rate-limit caps to flagged devices.

## Why this is open-source

These primitives derive their strength from hardware-rooted measurements + build-time pinning + clever architecture (silent-poison), not from source secrecy. A determined attacker reverse-engineering a closed binary would extract the same code in a few hours; closed-source obfuscation only delays casual analysis. For a hobbyist tool whose threat model is "stop malware distribution," the marginal benefit of obscurity isn't worth the contributor-friction cost (private repo, deploy-key rotation, two-crate stub/real split). See spec `docs/superpowers/specs/2026-05-04-guard-opensource-and-unified-versioning-design.md` for the full reasoning.

## Cargo features

- `serde` — derive `Serialize`/`Deserialize` on `DeviceId`. Required by callers that pass it across IPC boundaries.
- `strict-integrity` — replaces `option_env!` with `env!` for the `OMNI_GUARD_TEXT_SHA256` build constant. Release CI enables this so a missing hash fails the build instead of silently disabling the check. Dev builds leave it off (the env var is unset locally; the check no-ops).
- `dev-no-guard` — exposes `DisabledGuard` and lets the host's `make_guard()` return it. Use ONLY for local debugger sessions where the silent-poison anti-debug interferes with your debugging. Release builds MUST NOT enable this feature; the host's release `main` exits non-zero if `enforcement_mode() == Disabled`.

## Platform

Windows-only today. The `compile_error!` at `lib.rs` enforces this so a non-Windows build fails fast with a clear message.

## License

GPL-3.0. See workspace root `LICENSE`.
