use omni_shared::{OverlaySlot, SharedOverlayState, SHARED_MEM_NAME};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_ALL_ACCESS,
};

use crate::logging::log_to_file;

pub struct SharedMemoryReader {
    handle: HANDLE,
    ptr: *mut SharedOverlayState,
    last_sequence: u64,
}

// SAFETY: SharedMemoryReader is only used on the render thread. Send+Sync
// are required because it's stored in a static mut, but actual access is
// single-threaded.
unsafe impl Send for SharedMemoryReader {}
unsafe impl Sync for SharedMemoryReader {}

impl SharedMemoryReader {
    /// Try to open the existing named shared memory created by the host.
    /// Returns None if the shared memory doesn't exist yet (host not running).
    pub fn open() -> Option<Self> {
        let name_wide: Vec<u16> = SHARED_MEM_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // SAFETY: Opening an existing named file mapping created by the host.
        // name_wide is a valid null-terminated UTF-16 string on the stack.
        let handle = unsafe {
            OpenFileMappingW(
                FILE_MAP_ALL_ACCESS.0,
                false,
                windows::core::PCWSTR(name_wide.as_ptr()),
            )
        };

        let handle = match handle {
            Ok(h) => h,
            Err(_) => return None, // Host hasn't created shared memory yet
        };

        // SAFETY: handle was successfully opened. FILE_MAP_ALL_ACCESS matches
        // the host's PAGE_READWRITE protection.
        let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0) };

        if ptr.Value.is_null() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return None;
        }

        log_to_file("[ipc] shared memory opened successfully");

        Some(Self {
            handle,
            ptr: ptr.Value as *mut SharedOverlayState,
            last_sequence: 0,
        })
    }

    /// Read the active slot. Returns None if data hasn't changed since last read.
    pub fn read(&mut self) -> Option<&OverlaySlot> {
        // SAFETY: self.ptr points to valid shared memory mapped in open.
        // The host writes to the inactive slot and atomically flips.
        let state = unsafe { &*self.ptr };
        let slot_idx = state.reader_slot_index();
        let slot = &state.slots[slot_idx];

        if slot.write_sequence == self.last_sequence {
            return None; // No new data
        }

        self.last_sequence = slot.write_sequence;
        Some(slot)
    }

    /// Read the active slot unconditionally (even if sequence hasn't changed).
    pub fn read_current(&self) -> &OverlaySlot {
        // SAFETY: self.ptr points to valid shared memory mapped in open.
        // The host writes to the inactive slot and atomically flips.
        let state = unsafe { &*self.ptr };
        let slot_idx = state.reader_slot_index();
        &state.slots[slot_idx]
    }

    /// Returns true if the host appears to be writing (sequence > 0).
    pub fn is_connected(&self) -> bool {
        let state = unsafe { &*self.ptr };
        let slot = &state.slots[state.reader_slot_index()];
        slot.write_sequence > 0
    }

    /// Write frame data back to shared memory so the host can use
    /// FPS/frame-time values in reactive class conditions.
    ///
    /// # Safety
    /// Only the render thread calls this — no concurrent writes.
    pub unsafe fn write_frame_data(&self, frame_data: &omni_shared::FrameData) {
        let state = &mut *self.ptr;
        state.dll_frame_data = *frame_data;
    }
}

impl Drop for SharedMemoryReader {
    fn drop(&mut self) {
        // SAFETY: Unmapping the view and closing the handle we opened.
        unsafe {
            let _ = UnmapViewOfFile(windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr as *mut std::ffi::c_void,
            });
            let _ = CloseHandle(self.handle);
        }
        log_to_file("[ipc] shared memory closed");
    }
}
