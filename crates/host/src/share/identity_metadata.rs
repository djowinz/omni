//! Persistent metadata for the active identity. Mirrors what the worker
//! has about us (`display_name`) plus local-only state the worker
//! doesn't need to know (`backed_up`, `last_backup_path`).
//!
//! On-disk path: `%APPDATA%\Omni\identity-metadata.json` (resolved by
//! the caller — this module only takes a `&Path`).
//!
//! Source-of-truth tripwire: the `pubkey_hex` field is checked on load
//! against the active `Keypair`. Mismatch → reset to defaults so we
//! never return a wrong `backed_up` answer for a different key. The
//! reset is persisted before returning so a subsequent load observes
//! the same state.
//!
//! Persistence uses [`crate::atomic::atomic_write`] to defend against
//! torn writes on power-loss; the parent dir is auto-created so first-
//! run callers don't need to pre-create `%APPDATA%\Omni\`.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../../packages/shared-types/src/generated/")]
pub struct IdentityMetadata {
    /// Hex-encoded public key the rest of these fields belong to.
    /// Treated as the tripwire: on load, mismatch with the active key
    /// resets every other field to default (see [`Self::load_or_default`]).
    pub pubkey_hex: String,
    /// User-chosen display name surfaced in `identity.show` and embedded
    /// in upload manifests. `None` until the user sets one.
    pub display_name: Option<String>,
    /// `true` once the user has confirmed they exported a backup of the
    /// active identity (`identity.backup` succeeded + UI ack). Cleared
    /// by the tripwire on rotation.
    pub backed_up: bool,
    /// Unix-seconds timestamp of the last successful backup operation.
    pub last_backed_up_at: Option<u64>,
    /// Unix-seconds timestamp of the last successful key rotation —
    /// surfaced by `identity.show` so the UI can flag a stale key.
    pub last_rotated_at: Option<u64>,
    /// Filesystem path of the most recent backup file, surfaced in
    /// `identity.show` so the renderer can offer "open last backup"
    /// affordances. Stored as a string (not `PathBuf`) so the JSON wire
    /// shape is platform-stable.
    pub last_backup_path: Option<String>,
}

impl IdentityMetadata {
    /// Load metadata from `path`, or return a fresh-defaults instance
    /// keyed to `current_pubkey_hex`. If the on-disk `pubkey_hex`
    /// doesn't match `current_pubkey_hex`, reset to defaults and
    /// persist the reset (tripwire).
    ///
    /// Tolerant of corrupt JSON, missing files, and unreadable bytes —
    /// any failure is treated identically to "no metadata yet" and
    /// returns the fresh defaults. This keeps the host startup path
    /// resilient to manual edits / partial writes from older builds.
    pub fn load_or_default(path: &Path, current_pubkey_hex: &str) -> Self {
        let on_disk: Option<Self> = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());

        match on_disk {
            Some(m) if m.pubkey_hex == current_pubkey_hex => m,
            _ => {
                let fresh = Self {
                    pubkey_hex: current_pubkey_hex.to_string(),
                    ..Default::default()
                };
                // Best-effort persist. If the write fails (read-only FS,
                // permission flake, etc.) the caller still observes a
                // consistent in-memory value — the next successful
                // `save` will catch the on-disk file up.
                let _ = Self::save(path, &fresh);
                fresh
            }
        }
    }

    /// Atomically persist `meta` to `path`. Creates the parent
    /// directory if missing so first-run callers don't need to
    /// pre-create `%APPDATA%\Omni\`.
    pub fn save(path: &Path, meta: &Self) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_vec_pretty(meta)
            .map_err(|e| std::io::Error::other(format!("serde: {e}")))?;
        crate::atomic::atomic_write(path, &json)
    }
}
