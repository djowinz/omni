//! Bitmap-based IPC protocol for shared memory between host and overlay DLL.
//!
//! The host renders the overlay to a BGRA bitmap via Ultralight and writes
//! it to shared memory. The DLL/overlay-exe reads the bitmap and blits it
//! to the game's back buffer.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

use crate::sensor_types::FrameData;

pub const BITMAP_SHM_NAME: &str = "OmniOverlay_Bitmap";
pub const BITMAP_IPC_VERSION: u32 = 1;

/// Maximum supported resolution (4K). Determines shared memory allocation size.
pub const MAX_WIDTH: u32 = 3840;
pub const MAX_HEIGHT: u32 = 2160;
/// Bytes per pixel (BGRA).
pub const BPP: u32 = 4;
/// Maximum pixel data size: 4K * 4 bytes = ~33 MB.
pub const MAX_PIXEL_DATA_SIZE: usize = (MAX_WIDTH * MAX_HEIGHT * BPP) as usize;

/// Fixed-size header at the start of shared memory.
#[repr(C)]
pub struct BitmapHeader {
    /// Protocol version -- must match BITMAP_IPC_VERSION.
    pub version: u32,
    /// Bitmap width in pixels.
    pub width: u32,
    /// Bitmap height in pixels.
    pub height: u32,
    /// Bytes per row (may include alignment padding).
    pub row_bytes: u32,
    /// Incremented on each write. DLL uses this for change detection.
    pub write_sequence: AtomicU64,
    /// Dirty rectangle -- the region that changed since last write.
    pub dirty_x: u32,
    pub dirty_y: u32,
    pub dirty_w: u32,
    pub dirty_h: u32,
    /// 1 = visible, 0 = hidden (hotkey toggle).
    pub overlay_visible: AtomicU8,
    /// Padding for alignment.
    _pad: [u8; 7],
    /// Frame data written by the DLL, read by the host.
    pub dll_frame_data: FrameData,
}

impl BitmapHeader {
    pub fn is_visible(&self) -> bool {
        self.overlay_visible.load(Ordering::Acquire) != 0
    }

    pub fn toggle_visible(&self) -> bool {
        let prev = self.overlay_visible.fetch_xor(1, Ordering::AcqRel);
        prev == 0
    }
}

/// Total shared memory size: header + max pixel buffer.
pub const TOTAL_SHM_SIZE: usize =
    std::mem::size_of::<BitmapHeader>() + MAX_PIXEL_DATA_SIZE;

/// Offset from the start of shared memory where pixel data begins.
pub const PIXEL_DATA_OFFSET: usize = std::mem::size_of::<BitmapHeader>();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_size_is_reasonable() {
        let size = std::mem::size_of::<BitmapHeader>();
        // Should be under 256 bytes
        assert!(size < 256, "BitmapHeader is {} bytes", size);
    }

    #[test]
    fn total_shm_size_is_under_64mb() {
        assert!(TOTAL_SHM_SIZE < 64 * 1024 * 1024);
    }

    #[test]
    fn visibility_toggle_works() {
        let header = BitmapHeader {
            version: BITMAP_IPC_VERSION,
            width: 0,
            height: 0,
            row_bytes: 0,
            write_sequence: AtomicU64::new(0),
            dirty_x: 0,
            dirty_y: 0,
            dirty_w: 0,
            dirty_h: 0,
            overlay_visible: AtomicU8::new(1),
            _pad: [0; 7],
            dll_frame_data: FrameData::default(),
        };
        assert!(header.is_visible());
        header.toggle_visible();
        assert!(!header.is_visible());
    }
}
