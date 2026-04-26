//! Writes rendered bitmap data to shared memory for the DLL/overlay-exe to read.

use std::ptr;
use std::sync::atomic::{AtomicU64, AtomicU8};

use shared::{
    total_shm_size, BitmapHeader, BITMAP_IPC_VERSION, BITMAP_SHM_NAME, PIXEL_DATA_OFFSET,
};
use tracing::{info, warn};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
};

pub struct BitmapWriter {
    handle: HANDLE,
    ptr: *mut u8,
    sequence: u64,
    pixel_capacity_bytes: usize,
}

unsafe impl Send for BitmapWriter {}

impl BitmapWriter {
    /// Create the bitmap shared-memory mapping with `pixel_capacity_bytes`
    /// reserved for pixel data. Caller is responsible for sizing capacity to
    /// the largest expected bitmap (typically max single-monitor area * BPP).
    pub fn create(pixel_capacity_bytes: usize) -> Result<Self, crate::error::HostError> {
        let total = total_shm_size(pixel_capacity_bytes);
        let total_u32: u32 = total.try_into().map_err(|_| {
            crate::error::HostError::Message(format!(
                "bitmap SHM size {} bytes exceeds u32 (CreateFileMappingW low-dword limit)",
                total
            ))
        })?;

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
                total_u32,
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

        unsafe {
            ptr::write_bytes(base, 0, total);
            let header = &mut *(base as *mut BitmapHeader);
            header.version = BITMAP_IPC_VERSION;
            header.overlay_visible = AtomicU8::new(1);
            header.write_sequence = AtomicU64::new(0);
            header.pixel_capacity_bytes = pixel_capacity_bytes as u64;
        }

        info!(
            total_bytes = total,
            pixel_capacity_bytes,
            name = BITMAP_SHM_NAME,
            "Bitmap shared memory created"
        );

        Ok(Self {
            handle,
            ptr: base,
            sequence: 0,
            pixel_capacity_bytes,
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
        let needed = (row_bytes as usize).saturating_mul(height as usize);
        if needed > self.pixel_capacity_bytes {
            warn!(
                needed,
                capacity = self.pixel_capacity_bytes,
                width,
                height,
                row_bytes,
                "bitmap exceeds SHM pixel capacity; frame dropped (monitor larger than expected at startup — restart host to resize SHM)"
            );
            return;
        }

        let header = unsafe { &mut *(self.ptr as *mut BitmapHeader) };

        header.width = width;
        header.height = height;
        header.row_bytes = row_bytes;
        header.dirty_x = dirty.0;
        header.dirty_y = dirty.1;
        header.dirty_w = dirty.2;
        header.dirty_h = dirty.3;

        let pixel_dest = unsafe { self.ptr.add(PIXEL_DATA_OFFSET) };
        if needed <= pixels.len() {
            unsafe {
                ptr::copy_nonoverlapping(pixels.as_ptr(), pixel_dest, needed);
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
