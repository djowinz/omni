//! Guard trait + deterministic `StubGuard` for dev builds.
//!
//! Per retro/2026-04-13-theme-sharing-004-design-retro.md (D-004-A), the
//! `Guard` trait is probe-only: it does not sign. Signing lives in
//! `omni-identity::Keypair`. Per D-004-E, this crate holds only the contract
//! (types + trait + stub impl), nothing else.

use std::fmt;

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceId(pub [u8; 32]);

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in &self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for DeviceId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for DeviceId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(DeviceId(<[u8; 32]>::deserialize(d)?))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    #[error("debugger detected")]
    DebuggerDetected,
    #[error("integrity check failed")]
    IntegrityFailed,
    #[error("vm environment detected")]
    VmDetected,
    #[error("other: {0}")]
    Other(String),
}

pub trait Guard: Send + Sync {
    fn device_id(&self) -> Result<DeviceId, GuardError>;
    fn verify_self_integrity(&self) -> Result<(), GuardError>;
    fn is_vm(&self) -> bool;
}

/// Deterministic in-repo stub. NEVER used in release builds with the real guard.
pub struct StubGuard;

impl Guard for StubGuard {
    fn device_id(&self) -> Result<DeviceId, GuardError> {
        let mut h = Sha256::new();
        h.update(b"dev-stub-device-id");
        Ok(DeviceId(h.finalize().into()))
    }

    fn verify_self_integrity(&self) -> Result<(), GuardError> {
        Ok(())
    }

    fn is_vm(&self) -> bool {
        false
    }
}
