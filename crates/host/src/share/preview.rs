//! `share::preview` — transient live-swap preview of the active overlay theme.
//!
//! Preview NEVER touches disk (no `AtomicDir`, no registry write, no TOFU
//! record). It snapshots the current theme bytes, applies candidate CSS via
//! the [`ThemeSwap`] seam, and schedules an auto-revert that fires on either
//! TTL expiry or explicit cancellation. At most one preview is live per host
//! session; a second `start` while one is active returns
//! [`PreviewError::PreviewActive`] (maps to the `PREVIEW_ACTIVE` wire code).
//!
//! The real wiring to `__omni_set_theme` lives in a later sub-spec; this
//! module is the seam + lifecycle only.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Minimal seam over the renderer's theme-swap surface.
///
/// The real impl wraps the Ultralight CSS-variable injection
/// (`__omni_set_theme`); tests substitute a recording double.
pub trait ThemeSwap: Send + Sync + 'static {
    /// Capture the currently-applied theme bytes for later revert.
    fn snapshot(&self) -> Vec<u8>;
    /// Apply the candidate CSS. Returns a string error on failure; the
    /// caller treats failure as terminal (no session stored).
    fn apply(&self, css: &[u8]) -> Result<(), String>;
    /// Replay a previously captured snapshot.
    fn revert(&self, snapshot: &[u8]) -> Result<(), String>;
}

#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error("preview session already active")]
    PreviewActive,
    /// No preview session is currently active. Added for #021 so the
    /// `explorer.cancelPreview` WS handler can distinguish "slot empty"
    /// from "token did not match" and surface distinct error codes.
    #[error("no active preview session")]
    NoActivePreview,
    /// A preview session is active but its token does not match the one
    /// supplied. Added for #021 so the `explorer.cancelPreview` WS handler
    /// can surface a distinct error code for the misuse case.
    #[error("preview token does not match active session")]
    TokenMismatch,
    #[error("failed to apply preview theme: {0}")]
    ApplyFailed(String),
}

/// Handle for a live preview session. The spawned auto-revert task is owned
/// by `_join`; dropping the session without cancellation is safe because the
/// `tokio::select!` arm on TTL will still fire and revert.
pub struct PreviewSession {
    token: Uuid,
    cancel: CancellationToken,
    _join: JoinHandle<()>,
}

impl PreviewSession {
    pub fn token(&self) -> Uuid {
        self.token
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }
}

/// Slot enforcing the "at most one preview per host session" invariant.
#[derive(Default)]
pub struct PreviewSlot {
    inner: Mutex<Option<PreviewSession>>,
}

impl PreviewSlot {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot, apply, and spawn the auto-revert task.
    ///
    /// On success, returns the preview token. On `apply` failure, returns
    /// [`PreviewError::ApplyFailed`] and leaves the slot empty.
    pub fn start<S: ThemeSwap>(
        &self,
        swap: Arc<S>,
        css: Vec<u8>,
        ttl: Duration,
    ) -> Result<Uuid, PreviewError> {
        let mut guard = self.inner.lock().expect("preview slot poisoned");
        if guard.is_some() {
            return Err(PreviewError::PreviewActive);
        }

        // Snapshot BEFORE apply so revert is always possible.
        let snapshot = swap.snapshot();
        swap.apply(&css).map_err(PreviewError::ApplyFailed)?;

        let token = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let task_swap = swap.clone();

        let join = tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(ttl) => {
                    let _ = task_swap.revert(&snapshot);
                }
                _ = task_cancel.cancelled() => {
                    let _ = task_swap.revert(&snapshot);
                }
            }
        });

        *guard = Some(PreviewSession {
            token,
            cancel,
            _join: join,
        });
        Ok(token)
    }

    /// Cancel an active preview by token. Added for #021 to support
    /// `explorer.cancelPreview` WS dispatch; integrates with #010's
    /// shipped `PreviewSlot`/`PreviewSession` pattern. The session's
    /// internal `CancellationToken` is fired, which wakes the
    /// auto-revert task in `start()` and restores the snapshot.
    ///
    /// Returns [`PreviewError::NoActivePreview`] when the slot is empty,
    /// or [`PreviewError::TokenMismatch`] when a session is active but its
    /// token does not match the one supplied (the session is preserved in
    /// the slot in that case).
    pub fn cancel(&self, token: Uuid) -> Result<(), PreviewError> {
        let mut guard = self.inner.lock().expect("preview slot poisoned");
        match guard.take() {
            Some(session) if session.token == token => {
                session.cancel.cancel();
                // Dropping the session here lets the spawned task finish on
                // its own; the select! arm on `cancelled()` has already
                // fired the revert.
                Ok(())
            }
            Some(other) => {
                // Token mismatch — put the session back, do not cancel.
                *guard = Some(other);
                Err(PreviewError::TokenMismatch)
            }
            None => Err(PreviewError::NoActivePreview),
        }
    }

    pub fn is_active(&self) -> bool {
        self.inner
            .lock()
            .expect("preview slot poisoned")
            .is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::task::yield_now;

    struct RecordSwap {
        applies: AtomicUsize,
        reverts: AtomicUsize,
    }

    impl RecordSwap {
        fn new() -> Self {
            Self {
                applies: AtomicUsize::new(0),
                reverts: AtomicUsize::new(0),
            }
        }
        fn applies(&self) -> usize {
            self.applies.load(Ordering::SeqCst)
        }
        fn reverts(&self) -> usize {
            self.reverts.load(Ordering::SeqCst)
        }
    }

    impl ThemeSwap for RecordSwap {
        fn snapshot(&self) -> Vec<u8> {
            b"snap".to_vec()
        }
        fn apply(&self, _css: &[u8]) -> Result<(), String> {
            self.applies.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn revert(&self, _snapshot: &[u8]) -> Result<(), String> {
            self.reverts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test(start_paused = true)]
    async fn ttl_expiry_auto_reverts() {
        let swap = Arc::new(RecordSwap::new());
        let slot = PreviewSlot::new();
        slot.start(swap.clone(), b"css".to_vec(), Duration::from_secs(5))
            .expect("start");
        assert_eq!(swap.applies(), 1);
        assert_eq!(swap.reverts(), 0);

        // Let the spawned select! task reach its sleep before advancing.
        for _ in 0..4 {
            yield_now().await;
        }
        tokio::time::advance(Duration::from_secs(6)).await;
        for _ in 0..4 {
            yield_now().await;
        }

        assert_eq!(swap.reverts(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn explicit_cancel_reverts_immediately() {
        let swap = Arc::new(RecordSwap::new());
        let slot = PreviewSlot::new();
        let token = slot
            .start(swap.clone(), b"css".to_vec(), Duration::from_secs(60))
            .expect("start");
        slot.cancel(token).expect("cancel");

        yield_now().await;
        yield_now().await;

        assert_eq!(swap.reverts(), 1);
    }

    #[tokio::test]
    async fn second_start_returns_preview_active() {
        let swap = Arc::new(RecordSwap::new());
        let slot = PreviewSlot::new();
        slot.start(swap.clone(), b"css".to_vec(), Duration::from_secs(60))
            .expect("first start");
        let err = slot
            .start(swap.clone(), b"css2".to_vec(), Duration::from_secs(60))
            .expect_err("second start must fail");
        assert!(matches!(err, PreviewError::PreviewActive));
    }

    #[tokio::test]
    async fn preview_slot_cancel_matching_token_ok() {
        let swap = Arc::new(RecordSwap::new());
        let slot = PreviewSlot::new();
        let token = slot
            .start(swap.clone(), b"css".to_vec(), Duration::from_secs(60))
            .expect("start");
        slot.cancel(token).expect("cancel with matching token ok");
        yield_now().await;
        yield_now().await;
        assert_eq!(swap.reverts(), 1);
        assert!(!slot.is_active());
    }

    #[tokio::test]
    async fn preview_slot_cancel_mismatched_token_returns_token_mismatch() {
        let swap = Arc::new(RecordSwap::new());
        let slot = PreviewSlot::new();
        slot.start(swap.clone(), b"css".to_vec(), Duration::from_secs(60))
            .expect("start");
        let err = slot
            .cancel(Uuid::new_v4())
            .expect_err("mismatched token must error");
        assert!(matches!(err, PreviewError::TokenMismatch));
        // Session stays in the slot — not cancelled.
        assert!(slot.is_active());
        assert_eq!(swap.reverts(), 0);
    }

    #[tokio::test]
    async fn preview_slot_cancel_empty_slot_returns_no_active_preview() {
        let slot = PreviewSlot::new();
        let err = slot
            .cancel(Uuid::new_v4())
            .expect_err("empty slot must error");
        assert!(matches!(err, PreviewError::NoActivePreview));
    }
}
