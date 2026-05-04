//! The real guard. Per retro D-004-B, construction runs the integrity check.

use crate::integrity;
use crate::types::{DeviceId, GuardError};
use crate::Guard;

pub struct RealGuard {
    _private: (),
}

impl RealGuard {
    pub fn new() -> Result<Self, GuardError> {
        integrity::verify_text_section()?;
        Ok(Self { _private: () })
    }
}

impl Guard for RealGuard {
    fn device_id(&self) -> Result<DeviceId, GuardError> {
        Ok(DeviceId(crate::device::telemetry_session_seed()))
    }

    fn verify_self_integrity(&self) -> Result<(), GuardError> {
        integrity::verify_text_section()
    }

    fn is_vm(&self) -> bool {
        crate::vm::is_vm()
    }
}
