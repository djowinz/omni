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

pub mod cache;
pub mod client;
pub mod error;
pub mod handlers;
pub mod identity_metadata;
pub mod install;
pub mod preview;
pub mod preview_impl;
pub mod progress;
pub mod registry;
pub mod thumbnail;
pub mod tofu;
pub mod upload;
pub mod ws_messages;

pub use error::{UploadError, WorkerErrorKind};
