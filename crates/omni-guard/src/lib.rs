//! omni-guard — abuse-prevention primitives for Omni.
//!
//! Probe-only per retro D-004-A: device fingerprinting, anti-debug, VM
//! detection, PE .text self-integrity. No signing, no keys (signing lives
//! in `identity::Keypair`). Open-sourced 2026-05-04 from the private
//! `djowinz/omni-guard` repo; see
//! `docs/superpowers/specs/2026-05-04-guard-opensource-and-unified-versioning-design.md`
//! for context.

#[cfg(not(target_os = "windows"))]
compile_error!("omni-guard requires Windows; the app is Windows-only today");

mod antidebug;
mod device;
mod integrity;
mod real;
mod traits;
mod types;
mod vm;

#[cfg(any(test, feature = "dev-no-guard"))]
mod disabled;

pub use real::RealGuard;
pub use traits::Guard;
pub use types::{DeviceId, EnforcementMode, GuardError};

#[cfg(any(test, feature = "dev-no-guard"))]
pub use disabled::DisabledGuard;
