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
pub mod handlers;
pub mod install;
pub mod preview;
pub mod progress;
pub mod registry;
pub mod tofu;
pub mod upload;
pub mod ws_messages;

pub use error::{UploadError, WorkerErrorKind};

/// Host-side install/preview dependency bundle.
///
/// Construction + WS-thread accessibility lands in the post-Phase-2
/// async-bridge chore in `ws_server.rs`. Until that lands, every
/// `explorer.*` dispatcher arm sees `None` here and returns the
/// `service_unavailable` D-004-J envelope. Mirrors sub-spec #009's
/// `ShareContext` pattern so the bridge refactor wires both at once.
pub struct InstallContext {
    // Fields intentionally empty for the stub phase. The bridge chore
    // will populate: ShareClient, TofuStore, RegistryHandle (themes),
    // RegistryHandle (bundles), PreviewSlot, tokio runtime Handle,
    // progress-mpsc Sender — all behind interior mutability as needed.
}
