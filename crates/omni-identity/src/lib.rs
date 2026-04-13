//! Omni local identity: Ed25519 keypair, fingerprint, encrypted backup, TOFU registry.
//!
//! All byte layouts are authoritative per
//! `docs/superpowers/specs/contracts/identity-file-format.md`.

mod atomic;
mod emojilist;
mod error;
mod fingerprint;
mod format;
mod keypair;
mod tofu;
mod wordlist;

#[cfg(windows)]
mod acl;

pub use error::IdentityError;
// Re-exports below are enabled as their modules are implemented in later tasks.
pub use fingerprint::{Fingerprint, PublicKey};
// pub use keypair::Keypair;
pub use tofu::{TofuEntry, TofuRegistry, TofuResult};
