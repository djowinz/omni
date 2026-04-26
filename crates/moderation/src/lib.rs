//! Local NSFW moderation for Omni's upload pipeline.
//!
//! Wraps a bundled NudeNet ONNX detector behind an `ort`-based session that
//! the host loads once on startup and reuses across renderer-initiated
//! Preview Image checks (INV-7.7.2 site #1) and pack-time Dependency Check
//! image scans (INV-7.7.2 site #2). The threshold for "unsafe" rejection is
//! `0.8` (INV-7.7.3); callers compare against [`ModerationResult::unsafe_score`].
//!
//! Crate boundary stays format/inference only — caller (host) is responsible
//! for: routing decisions, logging, surfacing rejection chrome, and threshold
//! comparisons. See spec §8.5.

pub mod nudenet;

pub use nudenet::{ModerationError, ModerationResult, NudeNetModel};
