//! Omni bundle manifest + pack/unpack.
//!
//! See `docs/superpowers/specs/2026-04-10-theme-sharing-005-omni-bundle.md`.

mod error;
mod hash;
mod manifest;
mod pack;
mod path;
mod unpack;

pub use error::{BundleError, IntegrityKind, UnsafeKind};
pub use hash::canonical_hash;
pub use manifest::{FileEntry, Manifest, ResourceKind, Tag};
pub use pack::pack;
pub use unpack::unpack;

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
