//! Guard trait + deterministic `StubGuard` for dev builds.

use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceId(pub [u8; 32]);

#[derive(Debug, Clone, Copy)]
pub struct Signature(pub [u8; 64]);

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
    fn sign(&self, payload: &[u8]) -> Result<Signature, GuardError>;
    fn verify_self_integrity(&self) -> Result<(), GuardError>;
    fn is_vm(&self) -> bool;
}

/// Deterministic in-repo stub. NEVER used in release builds with the real guard.
pub struct StubGuard;

/// Hardcoded dev-only seed. Documented as insecure — real builds use `omni-guard`.
const STUB_SEED: [u8; 32] = [
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
    0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10,
    0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18,
    0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20,
];

impl Guard for StubGuard {
    fn device_id(&self) -> Result<DeviceId, GuardError> {
        let mut h = Sha256::new();
        h.update(b"dev-stub-device-id");
        Ok(DeviceId(h.finalize().into()))
    }

    fn sign(&self, payload: &[u8]) -> Result<Signature, GuardError> {
        use ed25519_dalek::{Signer, SigningKey};
        let sk = SigningKey::from_bytes(&STUB_SEED);
        let sig = sk.sign(payload);
        Ok(Signature(sig.to_bytes()))
    }

    fn verify_self_integrity(&self) -> Result<(), GuardError> {
        Ok(())
    }

    fn is_vm(&self) -> bool {
        false
    }
}
