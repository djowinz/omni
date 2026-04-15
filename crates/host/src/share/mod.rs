//! Host-side upload pipeline (sub-spec #009).
//!
//! Public surface of this module is consumed by `crate::ws_server` via
//! [`ws_messages::dispatch`]. Internal structure:
//!
//! * [`error`] — domain-carved `UploadError` (invariant #19a).
//! * [`progress`] — `UploadProgress` enum + WS adapter.
//! * [`cache`] — post-upload cache papering over D1/KV eventual consistency.
//! * [`client`] — `ShareClient` (reqwest + JWS middleware).
//! * [`upload`] — `pack_only` + `upload` orchestration.
//! * [`ws_messages`] — WebSocket dispatch.

pub mod cache;
pub mod client;
pub mod error;
pub mod progress;
pub mod upload;
pub mod ws_messages;

pub use error::{UploadError, WorkerErrorKind};
