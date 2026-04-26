//! Host-side share pipeline.
//!
//! Per architectural invariant #12 the host is the single boundary between
//! editor and Worker. All outbound/inbound-Worker concerns live under this
//! module. Internal structure:
//!
//! * [`error`] — domain-carved `UploadError` (invariant #19a).
//! * [`progress`] — `UploadProgress` enum + WS adapter.
//! * [`cache`] — post-upload cache papering over D1/KV eventual consistency.
//! * [`client`] — `ShareClient` (reqwest + JWS middleware).
//! * [`upload`] — `pack_only` + `upload` orchestration.
//! * [`ws_messages`] — WebSocket dispatch.
//! * [`thumbnail`] — thumbnail rendering for themes and bundles (sub-spec #011).
//! * [`preview_impl`] — real `ThemeSwap` that drives `__omni_set_theme` via
//!   a pending-slot drained by the main render loop (phase-2 followup #3).
//! * [`sidecar`] — `.omni-publish.json` per-overlay/per-theme I/O (upload-flow
//!   redesign Wave A0; spec §8.1).
//! * [`publish_index`] — workspace-global publish-index used as silent-restore
//!   source for missing sidecars (upload-flow redesign Wave A0; spec §8.2).
//! * [`save_preview`] — save-time `.omni-preview.png` rendering. Wraps the
//!   existing `share::thumbnail` pipeline with workspace-file entry points so
//!   the post-`file.write` hook can render previews without unpacking a signed
//!   bundle (upload-flow redesign Wave A0; spec §8.3). Named `save_preview`
//!   to avoid colliding with the existing in-session theme-swap [`preview`]
//!   module.
//! * [`dep_resolver`] — overlay/theme/font/image dependency resolver +
//!   missing-refs / unused-files violation collector consumed by the
//!   Step 3 Dependency Check stage (upload-flow redesign Wave A1; spec
//!   §8.4 + INV-7.8.\*).

pub mod cache;
pub mod client;
pub mod dep_resolver;
pub mod error;
pub mod handlers;
pub mod identity_metadata;
pub mod install;
pub mod preview;
pub mod preview_impl;
pub mod progress;
pub mod publish_index;
pub mod registry;
pub mod save_preview;
pub mod sidecar;
pub mod thumbnail;
pub mod tofu;
pub mod upload;
pub mod ws_messages;

pub use error::{UploadError, WorkerErrorKind};
