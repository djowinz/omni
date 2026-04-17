//! Omni bundle format: `.omnipkg` pack/unpack, canonical hashing, manifest types.
//!
//! This crate is **format-only and crypto-free**. Per `omni-architecture-invariants`
//! rules 4 and 6a:
//!
//! - **Signing / verification** lives in `omni-identity` (`pack_signed_bundle`,
//!   `unpack_signed_bundle`). `omni_bundle::pack` / `unpack` are low-level
//!   primitives that every other sub-system consumes *through* `omni-identity`.
//! - **Per-kind content validation** (what CSS is valid, what PNG bytes are
//!   valid, executable magic-byte deny-list) lives in `omni-sanitize`. This
//!   crate only enforces universal path safety, size limits, and structural
//!   integrity (hashes match manifest).
//!
//! See `docs/contracts/canonical-hash-algorithm.md` for the
//! authoritative hash algorithm spec (schema_version = 1).

mod error;
mod hash;
mod manifest;
mod pack;
mod path;
mod unpack;

#[cfg(feature = "wasm")]
pub mod wasm;

pub use error::{BundleError, IntegrityKind, UnsafeKind};
pub use hash::canonical_hash;
pub use manifest::{FileEntry, Manifest, ResourceKind, Tag};
pub use pack::pack;
pub use unpack::{unpack, unpack_manifest, Unpack, UnpackedFile};

// ---------- Security invariants (compile-time; changes require security review) ----------

/// SECURITY INVARIANT — do not change without coordinated security review and version bump.
pub const MAX_PATH_DEPTH: usize = 2;
/// SECURITY INVARIANT — do not change without coordinated security review and version bump.
pub const MAX_COMPRESSION_RATIO: u64 = 100;
/// SECURITY INVARIANT — do not change without coordinated security review and version bump.
pub const MAX_PATH_LENGTH: usize = 100;

// ---------- Policy limits (runtime; fetched from Worker config:limits KV) ----------

/// Runtime-configurable size-policy limits. Per retro-005 D7, the Worker's
/// `config:limits` KV is the authority; callers fetch current values and
/// pass them into pack/unpack. `BundleLimits::DEFAULT` exists for local dev
/// and unit tests where no Worker is available.
#[derive(Debug, Clone, Copy)]
pub struct BundleLimits {
    pub max_bundle_compressed: u64,
    pub max_bundle_uncompressed: u64,
    pub max_entries: usize,
}

impl BundleLimits {
    /// Conservative defaults matching the shipped values. Use for local work
    /// where Worker policy is not available.
    pub const DEFAULT: BundleLimits = BundleLimits {
        max_bundle_compressed: 5 * 1024 * 1024,
        max_bundle_uncompressed: 10 * 1024 * 1024,
        max_entries: 32,
    };
}
