//! Disabled guard for development-only use. Exposed via the `dev-no-guard`
//! cargo feature OR `#[cfg(test)]`; release builds without the feature
//! literally do not contain this struct. Renamed from the historical
//! `StubGuard` to reflect its actual semantic.

use sha2::{Digest, Sha256};

use crate::types::{DeviceId, EnforcementMode, GuardError};
use crate::Guard;

pub struct DisabledGuard;

impl Guard for DisabledGuard {
    fn device_id(&self) -> Result<DeviceId, GuardError> {
        let mut h = Sha256::new();
        h.update(b"omni-guard-disabled");
        Ok(DeviceId(h.finalize().into()))
    }

    fn verify_self_integrity(&self) -> Result<(), GuardError> {
        Ok(())
    }

    fn is_vm(&self) -> bool {
        false
    }

    fn enforcement_mode(&self) -> EnforcementMode {
        EnforcementMode::Disabled
    }
}
