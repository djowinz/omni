/// IPC protocol types for shared memory between host and overlay DLL.
/// Uses a lock-free double buffer: host writes to inactive slot,
/// atomically flips active_slot, DLL reads from active slot.
use std::sync::atomic::{AtomicU64, Ordering};

use crate::sensor_types::SensorSnapshot;
use crate::widget_types::ComputedWidget;

pub const SHARED_MEM_NAME: &str = "OmniOverlay_SharedState";
pub const CONTROL_PIPE_NAME: &str = r"\\.\pipe\OmniOverlay_Control";
pub const MAX_WIDGETS: usize = 64;

/// Protocol version. Bump when SharedOverlayState layout changes.
/// Host writes this on creation; DLL checks it on open.
pub const IPC_PROTOCOL_VERSION: u32 = 1;

#[repr(C)]
pub struct SharedOverlayState {
    /// Protocol version — must match IPC_PROTOCOL_VERSION on both sides.
    pub version: u32,
    /// 0 or 1 — which slot the DLL should read from.
    pub active_slot: AtomicU64,
    pub slots: [OverlaySlot; 2],
    /// Frame data written by the DLL, read by the host.
    /// This enables the host to use FPS/frame-time in reactive class conditions.
    pub dll_frame_data: crate::sensor_types::FrameData,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct OverlaySlot {
    /// Incremented by host on each write. DLL can use this to detect updates.
    pub write_sequence: u64,
    pub sensor_data: SensorSnapshot,
    /// Bumped when widget config/style changes (not on every sensor update).
    pub layout_version: u64,
    pub widget_count: u32,
    pub widgets: [ComputedWidget; MAX_WIDGETS],
}

impl SharedOverlayState {
    /// Returns the index (0 or 1) of the slot the DLL should read.
    pub fn reader_slot_index(&self) -> usize {
        // Acquire ensures all host writes to the slot are visible before the DLL reads it.
        // `& 1` masks to a valid slot index regardless of counter wrap.
        self.active_slot.load(Ordering::Acquire) as usize & 1
    }

    /// Returns the index (0 or 1) of the slot the host should write to.
    pub fn writer_slot_index(&self) -> usize {
        // Acquire ordering mirrors reader_slot_index; XOR flips to the opposite slot
        // so the host always writes to the slot the DLL is not currently reading.
        (self.active_slot.load(Ordering::Acquire) as usize & 1) ^ 1
    }

    /// Host calls this after writing to the writer slot to make it active.
    pub fn flip_slot(&self) {
        // Release ordering ensures all writes to the new slot are visible to the DLL
        // before the index change is published. Only the host thread calls this, so
        // no compare-exchange is needed.
        let current = self.active_slot.load(Ordering::Acquire);
        let next = current ^ 1;
        self.active_slot.store(next, Ordering::Release);
    }
}

impl Default for OverlaySlot {
    fn default() -> Self {
        Self {
            write_sequence: 0,
            sensor_data: SensorSnapshot::default(),
            layout_version: 0,
            widget_count: 0,
            widgets: [ComputedWidget::default(); MAX_WIDGETS],
        }
    }
}

/// Control messages sent over the named pipe.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlMessage {
    /// Host tells DLL to shut down gracefully.
    Shutdown = 0,
    /// Host tells DLL that config has been reloaded.
    ConfigReloaded = 1,
    /// DLL reports hooks installed successfully.
    HooksInstalled = 128,
    /// DLL reports an error. Followed by a null-terminated error string.
    Error = 129,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn shared_state_size_is_stable() {
        let size = mem::size_of::<SharedOverlayState>();
        // Two OverlaySlots + AtomicU64 — should be substantial
        assert!(
            size > 1000,
            "SharedOverlayState is unexpectedly small: {size}"
        );
    }

    #[test]
    fn slot_flip_toggles_between_0_and_1() {
        let state = SharedOverlayState {
            version: IPC_PROTOCOL_VERSION,
            active_slot: AtomicU64::new(0),
            slots: [OverlaySlot::default(), OverlaySlot::default()],
            dll_frame_data: crate::sensor_types::FrameData::default(),
        };

        assert_eq!(state.reader_slot_index(), 0);
        assert_eq!(state.writer_slot_index(), 1);

        state.flip_slot();

        assert_eq!(state.reader_slot_index(), 1);
        assert_eq!(state.writer_slot_index(), 0);

        state.flip_slot();

        assert_eq!(state.reader_slot_index(), 0);
        assert_eq!(state.writer_slot_index(), 1);
    }

    #[test]
    fn overlay_slot_default_is_zeroed() {
        let slot = OverlaySlot::default();
        assert_eq!(slot.write_sequence, 0);
        assert_eq!(slot.layout_version, 0);
        assert_eq!(slot.widget_count, 0);
    }
}
