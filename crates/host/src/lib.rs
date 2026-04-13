//! omni-host library entry — the binary (`main.rs`) re-exports these
//! modules via `use omni_host::...`. Integration tests in
//! `crates/host/tests/*.rs` consume the same library surface.

pub mod config;
pub mod error;
pub mod etw;
pub mod hotkey;
pub mod ipc;
pub mod omni;
pub mod scanner;
pub mod sensors;
pub mod ul_renderer;
pub mod watcher;
pub mod win32;
pub mod workspace;
pub mod ws_server;
