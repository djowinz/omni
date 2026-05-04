use std::fmt;

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

/// Whether the active guard is providing real anti-tamper protection or
/// is intentionally disabled for development. Release main refuses to
/// start when the active guard returns `Disabled`. Defense-in-depth: the
/// structural guarantee comes from `DisabledGuard` being gated
/// `#[cfg(any(test, feature = "dev-no-guard"))]`, so a release binary
/// without that feature literally doesn't contain the variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementMode {
    Real,
    Disabled,
}
