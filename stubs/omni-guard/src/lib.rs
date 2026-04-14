//! STUB API MIRROR — keep in sync with private omni-guard.
//!
//! Committed to the public repo so cargo's workspace resolver can satisfy
//! `omni-host`'s optional `omni-guard` dep without cloning the private
//! repository. Retro D-004-K; see
//! `docs/superpowers/omni-architecture-invariants.md` invariant #14.
//!
//! The stub's public API surface MUST match the real crate's — anything
//! `omni-host` calls must compile here too. The stub always returns
//! `StubGuard`-equivalent behavior so `cargo test -p omni-host --features
//! guard` on a default checkout still produces a working guard (no real
//! fingerprinting, no anti-debug, no integrity).
//!
//! Release CI overrides this via `cargo --config 'patch."ssh://..."
//! .omni-guard={ git = "...", branch = "main" }'` to resolve against the
//! real private crate instead.

use omni_guard_trait::{DeviceId, Guard, GuardError, StubGuard};

pub struct RealGuard(StubGuard);

impl RealGuard {
    pub fn new() -> Result<Self, GuardError> {
        Ok(Self(StubGuard))
    }
}

impl Guard for RealGuard {
    fn device_id(&self) -> Result<DeviceId, GuardError> {
        self.0.device_id()
    }

    fn verify_self_integrity(&self) -> Result<(), GuardError> {
        self.0.verify_self_integrity()
    }

    fn is_vm(&self) -> bool {
        self.0.is_vm()
    }
}
