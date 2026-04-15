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

// Native-only modules: depend on `jsonwebtoken` (→ `ring`), which does not
// cross-compile to `wasm32-unknown-unknown` without a C toolchain. The WASM
// build exposes its own sign/verify/pack/unpack in `wasm` module below.
#[cfg(not(target_arch = "wasm32"))]
mod bundle;

#[cfg(windows)]
mod acl;

// Pure-Rust algorithmic core of the wasm JWS signer. Split out so the byte
// layout of the signed envelope is inspectable independent of the wasm-bindgen
// surface. Byte-for-byte equivalence with the native `jsonwebtoken` path is
// enforced by `tests/jws_native_wasm_parity.rs`, which re-implements this
// algorithm inline and diffs both against the native oracle.
#[cfg(feature = "wasm")]
pub(crate) mod wasm_jws_core;

#[cfg(feature = "wasm")]
pub mod wasm;

#[cfg(not(target_arch = "wasm32"))]
pub use bundle::{pack_signed_bundle, unpack_signed_bundle, SignedBundle};
pub use error::IdentityError;
// Re-exports below are enabled as their modules are implemented in later tasks.
pub use fingerprint::{Fingerprint, PublicKey};
#[cfg(not(target_arch = "wasm32"))]
pub use keypair::verify_jws;
pub use keypair::Keypair;
#[cfg(not(target_arch = "wasm32"))]
pub mod http_jws;
#[cfg(not(target_arch = "wasm32"))]
pub use http_jws::{sign_http_jws, HttpJwsClaims};
pub use tofu::{TofuEntry, TofuRegistry, TofuResult};
