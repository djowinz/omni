//! Writes rendered bitmap data to shared memory for the DLL/overlay-exe to read.

use std::ptr;
use std::sync::atomic::{AtomicU64, AtomicU8};

use omni_shared::{
    BitmapHeader, BITMAP_IPC_VERSION, BITMAP_SHM_NAME, PIXEL_DATA_OFFSET, TOTAL_SHM_SIZE,
};
use tracing::info;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
};

pub struct BitmapWriter {
    handle: HANDLE,
    ptr: *mut u8,
    sequence: u64,
}

unsafe impl Send for BitmapWriter {}

impl BitmapWriter {
    pub fn create() -> Result<Self, crate::error::HostError> {
        let name_wide: Vec<u16> = BITMAP_SHM_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let handle = unsafe {
            CreateFileMappingW(
                HANDLE(-1isize as *mut std::ffi::c_void),
                None,
                PAGE_READWRITE,
                0,
                TOTAL_SHM_SIZE as u32,
                windows::core::PCWSTR(name_wide.as_ptr()),
            )
            .map_err(crate::error::HostError::Win32)?
        };

        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        if view.Value.is_null() {
            unsafe {
                let _ = CloseHandle(handle);
            }
            return Err(crate::error::HostError::Message(
                "MapViewOfFile returned null".into(),
            ));
        }

        let base = view.Value as *mut u8;

        // Zero-initialize and set version
        unsafe {
            ptr::write_bytes(base, 0, TOTAL_SHM_SIZE);
            let header = &mut *(base as *mut BitmapHeader);
            header.version = BITMAP_IPC_VERSION;
            header.overlay_visible = AtomicU8::new(1);
            header.write_sequence = AtomicU64::new(0);
        }

        info!(
            size = TOTAL_SHM_SIZE,
            name = BITMAP_SHM_NAME,
            "Bitmap shared memory created"
        );

        Ok(Self {
            handle,
            ptr: base,
            sequence: 0,
        })
    }

    /// Write a region of pixels to shared memory.
    /// `pixels` is BGRA, premultiplied alpha.
    /// `dirty` is (x, y, w, h) of the changed region.
    pub fn write(
        &mut self,
        width: u32,
        height: u32,
        row_bytes: u32,
        pixels: &[u8],
        dirty: (u32, u32, u32, u32),
    ) {
        let header = unsafe { &mut *(self.ptr as *mut BitmapHeader) };

        header.width = width;
        header.height = height;
        header.row_bytes = row_bytes;
        header.dirty_x = dirty.0;
        header.dirty_y = dirty.1;
        header.dirty_w = dirty.2;
        header.dirty_h = dirty.3;

        // Copy pixel data
        let pixel_dest = unsafe { self.ptr.add(PIXEL_DATA_OFFSET) };
        let copy_size = (row_bytes * height) as usize;
        if copy_size <= pixels.len() {
            unsafe {
                ptr::copy_nonoverlapping(pixels.as_ptr(), pixel_dest, copy_size);
            }
        }

        // Release fence ensures pixel data is visible before sequence update
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);

        self.sequence += 1;
        header
            .write_sequence
            .store(self.sequence, std::sync::atomic::Ordering::Release);
    }

    /// Returns a pointer to the header for hotkey visibility toggle.
    pub fn header_ptr(&self) -> *mut BitmapHeader {
        self.ptr as *mut BitmapHeader
    }
}

impl Drop for BitmapWriter {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr as *mut std::ffi::c_void,
            });
            let _ = CloseHandle(self.handle);
        }
        info!("Bitmap shared memory released");
    }
}
