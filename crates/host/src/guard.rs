//! Guard factory. Default builds use `StubGuard`; `--features guard` builds
//! use the private `RealGuard` consumed via a git URL dep.
//!
//! Per retro/2026-04-13-theme-sharing-004-design-retro.md:
//! - D-004-A: `Guard` has no `sign` method. All signing lives in
//!   `omni-identity::Keypair`; this factory takes no key.
//! - D-004-B: `RealGuard::new()` runs integrity at construction and returns
//!   `Result<Self, GuardError>`. `make_guard` propagates that.
//! - D-004-G: private crate is pulled via git URL, not via a submodule path.

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
