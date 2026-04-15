//! Thumbnail generation for themes and bundles.
//!
//! Composes the shipped Ultralight harness at `ViewTrust::ThumbnailGen`.
//! Never introduces a second renderer path (architectural invariant #8).
//!
//! Call order for bundles (invariants #6a + #19b):
//!   1. `omni_bundle::unpack_manifest`  (zero file I/O)
//!   2. schema_version + resource_kinds pre-flight
//!   3. `omni_identity::unpack_signed_bundle`
//!   4. stream `files()` to a `tempfile::TempDir`
//!   5. render via `render_omni_to_png`

use std::collections::HashMap;

use omni_bundle::BundleError;
use omni_identity::IdentityError;

/// Public error enum for thumbnail generation.
///
/// Carved on consumer semantics (invariant #19a). Third-party errors ride in
/// the `#[source]` chain rather than `#[from]`.
#[derive(Debug, thiserror::Error)]
pub enum ThumbnailError {
    #[error("bundle declares unsupported resource kind: {kind}")]
    UnsupportedKind { kind: String },

    #[error("bundle declares unsupported schema_version: {version}")]
    UnsupportedSchemaVersion { version: u32 },

    #[error("render failed: {detail}")]
    RenderFailed { detail: String },

    #[error("surface dimensions did not match configured size")]
    SurfaceDimensionsMismatch,

    #[error("encoded thumbnail exceeds size budget after retries: {bytes} bytes")]
    TooLarge { bytes: usize },

    #[error("identity error")]
    Identity(#[source] IdentityError),

    #[error("bundle error")]
    Bundle(#[source] BundleError),

    #[error("I/O error")]
    Io(#[source] std::io::Error),

    #[error("image encoding error")]
    Encode(#[source] image::ImageError),
}

/// Size budget from `contracts/worker-api.md` §4.1.
pub const MAX_THUMBNAIL_BYTES: usize = 256 * 1024;

/// Default render dimensions.
pub const DEFAULT_WIDTH: u32 = 800;
pub const DEFAULT_HEIGHT: u32 = 450;

/// Fallback dimensions used when the 800×450 PNG exceeds `MAX_THUMBNAIL_BYTES`
/// even under maximum PNG compression.
pub const FALLBACK_WIDTH: u32 = 600;
pub const FALLBACK_HEIGHT: u32 = 338;

#[derive(Debug, Clone)]
pub struct ThumbnailConfig {
    pub width: u32,
    pub height: u32,
    pub sample_values: HashMap<String, f64>,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        Self {
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            sample_values: default_sample_values(),
        }
    }
}

/// Deterministic sample-values. Spec §5 — values stay below any warn/crit
/// thresholds so untouched themes render in a neutral state.
pub fn default_sample_values() -> HashMap<String, f64> {
    HashMap::from([
        ("cpu.usage".into(), 42.0),
        ("cpu.temp".into(), 58.0),
        ("gpu.usage".into(), 67.0),
        ("gpu.temp".into(), 71.0),
        ("gpu.vram_used".into(), 6800.0),
        ("gpu.vram_total".into(), 8192.0),
        ("ram.used".into(), 16384.0),
        ("ram.total".into(), 32768.0),
        ("net.down".into(), 1200.0),
        ("net.up".into(), 340.0),
        ("fps.current".into(), 144.0),
    ])
}
