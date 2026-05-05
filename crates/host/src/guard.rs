//! Guard factory. Default builds use `RealGuard` (the real anti-tamper
//! primitives from `omni-guard`). Builds with `--features dev-no-guard`
//! return `DisabledGuard` instead — useful for local debugger sessions
//! where anti-debug interferes. Release builds MUST NOT enable
//! `dev-no-guard`; the host's release main checks
//! `guard.enforcement_mode()` and exits non-zero if it returns `Disabled`.

use omni_guard::{Guard, GuardError};

#[cfg(feature = "dev-no-guard")]
pub fn make_guard() -> Result<Box<dyn Guard>, GuardError> {
    tracing::warn!(
        "running with dev-no-guard feature; integrity + anti-debug + Sybil resistance disabled"
    );
    Ok(Box::new(omni_guard::DisabledGuard))
}

#[cfg(not(feature = "dev-no-guard"))]
pub fn make_guard() -> Result<Box<dyn Guard>, GuardError> {
    Ok(Box::new(omni_guard::RealGuard::new()?))
}
