use std::ptr;

use omni_shared::{
    SharedOverlayState, SensorSnapshot, ComputedWidget,
    SHARED_MEM_NAME, MAX_WIDGETS,
};
use tracing::info;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile,
    FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
};

pub struct SharedMemoryWriter {
    handle: HANDLE,
    ptr: *mut SharedOverlayState,
    sequence: u64,
}

// SAFETY: We control access — only the host thread writes.
unsafe impl Send for SharedMemoryWriter {}

impl SharedMemoryWriter {
    /// Create a new named shared memory region.
    pub fn create() -> Result<Self, crate::error::HostError> {
        let size = std::mem::size_of::<SharedOverlayState>() as u32;

        let name_wide: Vec<u16> = SHARED_MEM_NAME.encode_utf16().chain(std::iter::once(0)).collect();

        // SAFETY: INVALID_HANDLE_VALUE (-1) creates a page-file-backed mapping. name_wide is a valid null-terminated UTF-16 string.
        let handle = unsafe {
            CreateFileMappingW(
                HANDLE(-1isize as *mut std::ffi::c_void), // INVALID_HANDLE_VALUE = page file backed
                None,                                      // default security
                PAGE_READWRITE,
                0,
                size,
                windows::core::PCWSTR(name_wide.as_ptr()),
            )
            .map_err(crate::error::HostError::Win32)?
        };

        // SAFETY: handle was successfully created above. FILE_MAP_ALL_ACCESS grants read/write.
        let ptr = unsafe {
            MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0)
        };

        if ptr.Value.is_null() {
            unsafe { let _ = CloseHandle(handle); }
            return Err(crate::error::HostError::Message("MapViewOfFile returned null".into()));
        }

        let state_ptr = ptr.Value as *mut SharedOverlayState;

        // Zero-initialize the shared memory
        // SAFETY: state_ptr points to a freshly mapped region of the correct size. write_bytes zeroes it completely.
        unsafe {
            ptr::write_bytes(state_ptr, 0, 1);
            // Initialize active_slot to 0
            (*state_ptr).active_slot = std::sync::atomic::AtomicU64::new(0);
        }

        info!(size, name = SHARED_MEM_NAME, "Shared memory created");

        Ok(Self {
            handle,
            ptr: state_ptr,
            sequence: 0,
        })
    }

    /// Write sensor data and widgets to the inactive slot, then flip.
    pub fn write(&mut self, sensor_data: &SensorSnapshot, widgets: &[ComputedWidget], layout_version: u64) {
        // SAFETY: self.ptr is valid for the lifetime of this writer (mapped in create, unmapped in Drop). Only the host thread calls write.
        let state = unsafe { &*self.ptr };
        let slot_idx = state.writer_slot_index();
        let slot = unsafe { &mut (*self.ptr).slots[slot_idx] };

        self.sequence += 1;
        slot.write_sequence = self.sequence;
        slot.sensor_data = *sensor_data;
        slot.layout_version = layout_version;

        let count = widgets.len().min(MAX_WIDGETS);
        slot.widget_count = count as u32;
        slot.widgets[..count].copy_from_slice(&widgets[..count]);

        // Zero remaining widget slots
        for w in &mut slot.widgets[count..] {
            *w = ComputedWidget::default();
        }

        state.flip_slot();
    }

    /// Read frame data written by the DLL (FPS, frame time, etc.).
    /// Returns the DLL's frame stats so the host can use them in
    /// reactive class conditions (e.g., "fps < 30").
    pub fn read_dll_frame_data(&self) -> omni_shared::FrameData {
        // SAFETY: self.ptr is valid. Reading dll_frame_data is a plain Copy read.
        let state = unsafe { &*self.ptr };
        state.dll_frame_data
    }
}

impl Drop for SharedMemoryWriter {
    fn drop(&mut self) {
        // SAFETY: Unmapping and closing the handle we created in create.
        unsafe {
            let _ = UnmapViewOfFile(windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr as *mut std::ffi::c_void,
            });
            let _ = CloseHandle(self.handle);
        }
        info!("Shared memory released");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omni_shared::*;
    use std::sync::Mutex;

    // Tests share a global named shared memory region, so they must not run in parallel.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn create_and_write_shared_memory() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut writer = SharedMemoryWriter::create().expect("Failed to create shared memory");

        let snapshot = SensorSnapshot {
            timestamp_ms: 12345,
            cpu: CpuData {
                total_usage_percent: 42.5,
                core_count: 4,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut widget = ComputedWidget::default();
        widget.widget_type = WidgetType::SensorValue;
        widget.source = SensorSource::CpuUsage;
        widget.x = 10.0;
        widget.y = 10.0;
        write_fixed_str(&mut widget.format_pattern, "CPU: 42.5%");

        writer.write(&snapshot, &[widget], 1);

        // Read back from the now-active slot
        let state = unsafe { &*writer.ptr };
        let slot = &state.slots[state.reader_slot_index()];

        assert_eq!(slot.write_sequence, 1);
        assert_eq!(slot.sensor_data.timestamp_ms, 12345);
        assert_eq!(slot.sensor_data.cpu.total_usage_percent, 42.5);
        assert_eq!(slot.widget_count, 1);
        assert_eq!(slot.widgets[0].source, SensorSource::CpuUsage);
        assert_eq!(read_fixed_str(&slot.widgets[0].format_pattern), "CPU: 42.5%");
    }

    #[test]
    fn double_buffer_flips_correctly() {
        let _guard = TEST_LOCK.lock().unwrap();
        let mut writer = SharedMemoryWriter::create().expect("Failed to create shared memory");

        let snapshot1 = SensorSnapshot {
            timestamp_ms: 100,
            ..Default::default()
        };
        let snapshot2 = SensorSnapshot {
            timestamp_ms: 200,
            ..Default::default()
        };

        writer.write(&snapshot1, &[], 1);

        let state = unsafe { &*writer.ptr };
        let slot1 = &state.slots[state.reader_slot_index()];
        assert_eq!(slot1.sensor_data.timestamp_ms, 100);

        writer.write(&snapshot2, &[], 1);

        let slot2 = &state.slots[state.reader_slot_index()];
        assert_eq!(slot2.sensor_data.timestamp_ms, 200);
    }
}
