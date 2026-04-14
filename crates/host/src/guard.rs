//! Guard factory. Default builds use `StubGuard`; `--features guard` builds
//! use `omni_guard::RealGuard`, which resolves either to the public stub at
//! `stubs/omni-guard/` (default) or to the real private crate (release CI,
//! via a `--config` patch override).
//!
//! Per retro/2026-04-13-theme-sharing-004-design-retro.md:
//! - D-004-A: `Guard` has no `sign` method. All signing lives in
//!   `omni-identity::Keypair`; this factory takes no key.
//! - D-004-B: `RealGuard::new()` runs integrity at construction and returns
//!   `Result<Self, GuardError>`. `make_guard` propagates that.
//! - D-004-G: private crate is pulled via git URL, not via a submodule path.
//! - D-004-K: a public stub at `stubs/omni-guard/` satisfies the workspace
//!   resolver by default; the real crate replaces the stub only in release
//!   CI via `cargo --config` patch override.

use omni_guard_trait::{Guard, GuardError};

#[cfg(feature = "guard")]
pub fn make_guard() -> Result<Box<dyn Guard>, GuardError> {
    let real = omni_guard::RealGuard::new()?;
    Ok(Box::new(real))
}

#[cfg(not(feature = "guard"))]
pub fn make_guard() -> Result<Box<dyn Guard>, GuardError> {
    Ok(Box::new(omni_guard_trait::StubGuard))
}
