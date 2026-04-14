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
pub use manifest::{FileEntry, Manifest, Tag};
pub use pack::pack;
pub use unpack::unpack;

// ---------- Size / structural limits (authoritative) ----------

pub const MAX_BUNDLE_COMPRESSED: u64 = 5 * 1024 * 1024;
pub const MAX_BUNDLE_UNCOMPRESSED: u64 = 10 * 1024 * 1024;
pub const MAX_FONT: u64 = 1_572_864;
pub const MAX_IMAGE_RAW: u64 = 1_572_864;
pub const MAX_IMAGE_REENCODED: u64 = 1_048_576;
pub const MAX_CSS: u64 = 131_072;
pub const MAX_OVERLAY: u64 = 131_072;
pub const MAX_THEME_ONLY: u64 = 65_536;
pub const MAX_ENTRIES: usize = 32;
pub const MAX_PATH_DEPTH: usize = 2;
pub const MAX_COMPRESSION_RATIO: u64 = 100;
