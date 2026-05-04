use crate::types::{DeviceId, EnforcementMode, GuardError};

pub trait Guard: Send + Sync {
    fn device_id(&self) -> Result<DeviceId, GuardError>;
    fn verify_self_integrity(&self) -> Result<(), GuardError>;
    fn is_vm(&self) -> bool;
    /// Default `Real`. Only `DisabledGuard` overrides to `Disabled`. The
    /// release startup check in `host::main` exits non-zero when this
    /// returns `Disabled`.
    fn enforcement_mode(&self) -> EnforcementMode {
        EnforcementMode::Real
    }
}
