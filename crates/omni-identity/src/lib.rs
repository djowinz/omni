//! Omni local identity: Ed25519 keypair, fingerprint, encrypted backup, TOFU registry.
//!
//! All byte layouts are authoritative per
//! `docs/superpowers/specs/contracts/identity-file-format.md`.

mod atomic;
mod bundle;
mod emojilist;
mod error;
mod fingerprint;
mod format;
mod keypair;
mod tofu;
mod wordlist;

#[cfg(windows)]
mod acl;

pub use bundle::{pack_signed_bundle, unpack_signed_bundle, SignedBundle};
pub use error::IdentityError;
// Re-exports below are enabled as their modules are implemented in later tasks.
pub use fingerprint::{Fingerprint, PublicKey};
pub use keypair::{verify_jws, Keypair};
pub mod http_jws;
pub use http_jws::{sign_http_jws, HttpJwsClaims};
pub use tofu::{TofuEntry, TofuRegistry, TofuResult};
