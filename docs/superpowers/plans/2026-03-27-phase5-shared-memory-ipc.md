# Phase 5: Shared Memory IPC + First Sensor + D2D Renderer

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire host and overlay DLL together via shared memory IPC. Host polls CPU usage via `sysinfo`, writes fully-resolved widget data into a lock-free double-buffered shared memory region. DLL reads each frame and renders live sensor data using D2D1/DirectWrite, replacing the shader-based test quad.

**Architecture:** Host creates a named shared memory region (`CreateFileMappingW` + `MapViewOfFile`) sized to `SharedOverlayState`. A background thread polls `sysinfo` for CPU usage at 1s intervals, sends snapshots to the main loop via `mpsc` channel. The main loop builds a hardcoded `ComputedWidget` (CPU usage label), writes it to the inactive shared memory slot, and atomically flips. The DLL opens the shared memory lazily on first Present call, reads widgets, and renders via D2D1/DirectWrite. The existing shader-based renderer (`shaders.rs`, vertex buffers, blend state) is removed entirely.

**Tech Stack:** Rust, `windows` crate 0.58 (shared memory, D2D1, DirectWrite), `sysinfo` for CPU data, `tracing` for sensor binding ledger.

**Testing notes:** Shared memory creation/write/read is unit-testable within a single process. Sensor polling is testable against the real system. D2D rendering is tested manually in-game. The full pipeline is validated by seeing live CPU usage in the overlay.

**Depends on:** Phases 1–4 complete (injection, hooks, scanner, graceful shutdown all working).

---

## File Map

```
shared/
  Cargo.toml                         # No changes (no external deps)
  src/
    lib.rs                           # No changes
    ipc_protocol.rs                  # No changes (already defined)
    sensor_types.rs                  # No changes (already defined)
    widget_types.rs                  # No changes (already defined)

host/
  Cargo.toml                         # Add sysinfo, new windows features (Memory, IO)
  src/
    main.rs                          # Integrate sensor thread + shared memory writer into watch loop
    config.rs                        # No changes
    scanner.rs                       # No changes
    injector/
      mod.rs                         # No changes
    sensors/
      mod.rs                         # SensorPoller — background thread, mpsc sender
      cpu.rs                         # sysinfo-based CPU polling
    ipc/
      mod.rs                         # SharedMemoryWriter — create, write, flip

overlay-dll/
  Cargo.toml                         # Add D2D1, DirectWrite, Dxgi windows features
  src/
    lib.rs                           # Update omni_shutdown to release D2D resources
    hook.rs                          # No changes
    present.rs                       # Replace renderer with D2D, add shared memory reader
    logging.rs                       # No changes
    renderer.rs                      # REWRITE: D2D1/DirectWrite renderer
    shaders.rs                       # DELETE (replaced by D2D)
    state_backup.rs                  # No changes (still needed for D3D11 state save/restore)
    ipc/
      mod.rs                         # SharedMemoryReader — open, read active slot
```

---

### Task 1: Add Host Dependencies

**Files:**
- Modify: `host/Cargo.toml`

- [ ] **Step 1: Update host/Cargo.toml**

Add `sysinfo` and new `windows` features for shared memory and IO:

```toml
[dependencies]
omni-shared = { path = "../shared" }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ctrlc = "3"
sysinfo = "0.35"

[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_System_Threading",
    "Win32_System_Memory",
    "Win32_System_Diagnostics_Debug",
    "Win32_System_Diagnostics_ToolHelp",
    "Win32_System_LibraryLoader",
    "Win32_Security",
    "Win32_UI_WindowsAndMessaging",
    "Win32_System_IO",
]
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Downloads `sysinfo`, compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add host/Cargo.toml Cargo.lock
git commit -m "feat(host): add sysinfo dependency and IO windows feature"
```

---

### Task 2: CPU Sensor Poller

**Files:**
- Create: `host/src/sensors/mod.rs`
- Create: `host/src/sensors/cpu.rs`
- Modify: `host/src/main.rs` (add `mod sensors;`)

- [ ] **Step 1: Create host/src/sensors/cpu.rs**

```rust
use omni_shared::CpuData;
use sysinfo::System;
use tracing::info;

pub struct CpuPoller {
    system: System,
}

impl CpuPoller {
    pub fn new() -> Self {
        let mut system = System::new();
        system.refresh_cpu_all();

        let core_count = system.cpus().len();
        info!(core_count, "sysinfo: CPU sensor initialized");

        Self { system }
    }

    /// Refresh CPU data and return a populated CpuData struct.
    pub fn poll(&mut self) -> CpuData {
        self.system.refresh_cpu_all();

        let cpus = self.system.cpus();
        let core_count = cpus.len().min(32) as u32;

        let mut per_core_usage = [-1.0f32; 32];
        let mut per_core_freq_mhz = [0u32; 32];

        for (i, cpu) in cpus.iter().enumerate().take(32) {
            per_core_usage[i] = cpu.cpu_usage();
            per_core_freq_mhz[i] = cpu.frequency() as u32;
        }

        let total_usage = self.system.global_cpu_usage();

        CpuData {
            total_usage_percent: total_usage,
            per_core_usage,
            core_count,
            per_core_freq_mhz,
            package_temp_c: f32::NAN, // requires LHM/WMI, not available via sysinfo
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_poller_returns_valid_core_count() {
        let mut poller = CpuPoller::new();
        // First poll after new() may return 0% usage (sysinfo needs two samples)
        std::thread::sleep(std::time::Duration::from_millis(200));
        let data = poller.poll();
        assert!(data.core_count > 0, "Should detect at least one CPU core");
        assert!(data.core_count <= 32, "Core count should be capped at 32");
    }

    #[test]
    fn cpu_poller_unused_cores_are_negative() {
        let mut poller = CpuPoller::new();
        std::thread::sleep(std::time::Duration::from_millis(200));
        let data = poller.poll();
        // Cores beyond core_count should be -1.0
        for i in data.core_count as usize..32 {
            assert_eq!(data.per_core_usage[i], -1.0);
        }
    }

    #[test]
    fn cpu_poller_temp_is_nan() {
        let mut poller = CpuPoller::new();
        let data = poller.poll();
        assert!(data.package_temp_c.is_nan(), "Temp should be NaN without LHM");
    }
}
```

- [ ] **Step 2: Create host/src/sensors/mod.rs**

```rust
pub mod cpu;

use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use omni_shared::SensorSnapshot;
use tracing::error;

use cpu::CpuPoller;

/// Runs sensor polling on a background thread, sending snapshots via channel.
pub struct SensorPoller {
    handle: Option<thread::JoinHandle<()>>,
    running: Arc<AtomicBool>,
}

impl SensorPoller {
    /// Spawn the sensor polling thread. Returns the poller handle and a receiver
    /// for sensor snapshots.
    pub fn start(interval: Duration, running: Arc<AtomicBool>) -> (Self, mpsc::Receiver<SensorSnapshot>) {
        let (tx, rx) = mpsc::channel();
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            let mut cpu = CpuPoller::new();

            // sysinfo needs two samples to compute usage — wait before first real poll
            thread::sleep(Duration::from_millis(500));

            while running_clone.load(Ordering::Relaxed) {
                let cpu_data = cpu.poll();

                let snapshot = SensorSnapshot {
                    timestamp_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    cpu: cpu_data,
                    ..Default::default()
                };

                if tx.send(snapshot).is_err() {
                    break; // receiver dropped
                }

                thread::sleep(interval);
            }
        });

        (Self { handle: Some(handle), running }, rx)
    }

    /// Signal the polling thread to stop and wait for it to finish.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for SensorPoller {
    fn drop(&mut self) {
        self.stop();
    }
}
```

- [ ] **Step 3: Add mod declaration to main.rs**

Add `mod sensors;` after `mod scanner;` in `host/src/main.rs`.

- [ ] **Step 4: Verify it compiles and tests pass**

Run: `cargo test -p omni-host -- sensors`
Expected: 3 tests pass (core count, unused cores, temp NaN).

- [ ] **Step 5: Commit**

```bash
git add host/src/sensors/ host/src/main.rs
git commit -m "feat(host): add CPU sensor poller with sysinfo background thread"
```

---

### Task 3: Shared Memory Writer (Host)

**Files:**
- Create: `host/src/ipc/mod.rs`
- Modify: `host/src/main.rs` (add `mod ipc;`)

- [ ] **Step 1: Create host/src/ipc/mod.rs**

```rust
use std::ptr;

use omni_shared::{
    SharedOverlayState, OverlaySlot, SensorSnapshot, ComputedWidget,
    SHARED_MEM_NAME, MAX_WIDGETS,
};
use tracing::{info, error};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile,
    FILE_MAP_ALL_ACCESS, PAGE_READWRITE,
};
use windows::core::w;

pub struct SharedMemoryWriter {
    handle: HANDLE,
    ptr: *mut SharedOverlayState,
    sequence: u64,
}

// SAFETY: We control access — only the host thread writes.
unsafe impl Send for SharedMemoryWriter {}

impl SharedMemoryWriter {
    /// Create a new named shared memory region.
    pub fn create() -> Result<Self, String> {
        let size = std::mem::size_of::<SharedOverlayState>() as u32;

        let name_wide: Vec<u16> = SHARED_MEM_NAME.encode_utf16().chain(std::iter::once(0)).collect();

        let handle = unsafe {
            CreateFileMappingW(
                HANDLE(-1isize as *mut std::ffi::c_void), // INVALID_HANDLE_VALUE = page file backed
                None,                                      // default security
                PAGE_READWRITE,
                0,
                size,
                windows::core::PCWSTR(name_wide.as_ptr()),
            )
            .map_err(|e| format!("CreateFileMappingW failed: {e}"))?
        };

        let ptr = unsafe {
            MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0)
        };

        if ptr.Value.is_null() {
            unsafe { let _ = CloseHandle(handle); }
            return Err("MapViewOfFile returned null".into());
        }

        let state_ptr = ptr.Value as *mut SharedOverlayState;

        // Zero-initialize the shared memory
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
}

impl Drop for SharedMemoryWriter {
    fn drop(&mut self) {
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

    #[test]
    fn create_and_write_shared_memory() {
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
```

- [ ] **Step 2: Add mod declaration to main.rs**

Add `mod ipc;` after `mod sensors;` in `host/src/main.rs`.

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p omni-host -- ipc`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/ipc/ host/src/main.rs
git commit -m "feat(host): add shared memory writer with lock-free double buffer"
```

---

### Task 4: Integrate Sensor + Shared Memory into Watch Loop

**Files:**
- Modify: `host/src/main.rs`

- [ ] **Step 1: Update run_watch_mode in main.rs**

Replace the existing `run_watch_mode` function. The function now creates shared memory, starts the sensor poller, and writes to shared memory on each loop iteration alongside the scanner poll.

```rust
fn run_watch_mode(dll_path: &str) {
    let config_path = config::config_path();
    let config = config::load_config(&config_path);
    let poll_interval = Duration::from_millis(config.poll_interval_ms);

    ctrlc::set_handler(|| {
        RUNNING.store(false, Ordering::Relaxed);
    })
    .expect("Failed to set Ctrl+C handler");

    // Create shared memory for IPC with overlay DLL
    let mut shm_writer = match ipc::SharedMemoryWriter::create() {
        Ok(w) => w,
        Err(e) => {
            error!(error = %e, "Failed to create shared memory");
            std::process::exit(1);
        }
    };

    // Start sensor polling on background thread
    let (mut sensor_poller, sensor_rx) = sensors::SensorPoller::start(
        Duration::from_millis(1000),
        std::sync::Arc::new(AtomicBool::new(true)),
    );

    info!(
        dll_path,
        config_path = ?config_path,
        poll_ms = config.poll_interval_ms,
        exclude_count = config.exclude.len(),
        "Omni host starting in watch mode"
    );
    info!("Press Ctrl+C to stop");

    let mut scanner = scanner::Scanner::new(dll_path.to_string(), config);
    let mut latest_snapshot = omni_shared::SensorSnapshot::default();

    while RUNNING.load(Ordering::Relaxed) {
        scanner.poll();

        // Drain sensor channel — keep only the latest snapshot
        while let Ok(snapshot) = sensor_rx.try_recv() {
            latest_snapshot = snapshot;
        }

        // Build hardcoded CPU usage widget
        let widget = build_cpu_widget(&latest_snapshot);

        // Write to shared memory
        shm_writer.write(&latest_snapshot, &[widget], 1);

        std::thread::sleep(poll_interval);
    }

    info!("Shutting down — ejecting DLLs from injected processes");
    scanner.eject_all();
    sensor_poller.stop();
    info!("Omni host stopped");
}

/// Build a hardcoded CPU usage widget for this phase.
/// In later phases this is replaced by .widget file parsing + layout engine.
fn build_cpu_widget(snapshot: &omni_shared::SensorSnapshot) -> omni_shared::ComputedWidget {
    let mut widget = omni_shared::ComputedWidget::default();
    widget.widget_type = omni_shared::WidgetType::SensorValue;
    widget.source = omni_shared::SensorSource::CpuUsage;
    widget.x = 20.0;
    widget.y = 20.0;
    widget.width = 200.0;
    widget.height = 32.0;
    widget.font_size = 18.0;
    widget.font_weight = 700;
    widget.color_rgba = [255, 255, 255, 255]; // white text
    widget.bg_color_rgba = [20, 20, 20, 180]; // dark semi-transparent bg
    widget.border_radius = [4.0; 4];
    widget.opacity = 1.0;

    let text = format!("CPU: {:.0}%", snapshot.cpu.total_usage_percent);
    omni_shared::write_fixed_str(&mut widget.format_pattern, &text);

    widget
}
```

- [ ] **Step 2: Add necessary imports to main.rs**

Ensure these imports are at the top of `main.rs`:

```rust
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tracing::{info, error};
use tracing_subscriber::EnvFilter;
```

And all module declarations:

```rust
mod injector;
mod config;
mod scanner;
mod sensors;
mod ipc;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Compiles with no errors.

- [ ] **Step 4: Run all tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add host/src/main.rs
git commit -m "feat(host): integrate sensor polling and shared memory into watch loop"
```

---

### Task 5: Add DLL D2D/DirectWrite Dependencies

**Files:**
- Modify: `overlay-dll/Cargo.toml`

- [ ] **Step 1: Update overlay-dll/Cargo.toml**

Add D2D1, DirectWrite, and DXGI features:

```toml
[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_System_SystemServices",
    "Win32_System_LibraryLoader",
    "Win32_Graphics_Direct3D",
    "Win32_Graphics_Direct3D11",
    "Win32_Graphics_Dxgi",
    "Win32_Graphics_Dxgi_Common",
    "Win32_Graphics_Gdi",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Direct3D_Fxc",
    "Win32_Graphics_Direct2D",
    "Win32_Graphics_Direct2D_Common",
    "Win32_Graphics_DirectWrite",
    "Win32_System_Memory",
]
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-overlay-dll`
Expected: Compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/Cargo.toml Cargo.lock
git commit -m "feat(overlay-dll): add D2D1, DirectWrite, and shared memory windows features"
```

---

### Task 6: Shared Memory Reader (DLL)

**Files:**
- Create: `overlay-dll/src/ipc/mod.rs`
- Modify: `overlay-dll/src/lib.rs` (add `mod ipc;`)

- [ ] **Step 1: Create overlay-dll/src/ipc/mod.rs**

```rust
use std::ptr;

use omni_shared::{SharedOverlayState, OverlaySlot, SHARED_MEM_NAME};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    OpenFileMappingW, MapViewOfFile, UnmapViewOfFile,
    FILE_MAP_READ,
};

use crate::logging::log_to_file;

pub struct SharedMemoryReader {
    handle: HANDLE,
    ptr: *const SharedOverlayState,
    last_sequence: u64,
}

// SAFETY: Only one thread (render thread) reads from this.
unsafe impl Send for SharedMemoryReader {}
unsafe impl Sync for SharedMemoryReader {}

impl SharedMemoryReader {
    /// Try to open the existing named shared memory created by the host.
    /// Returns None if the shared memory doesn't exist yet (host not running).
    pub fn open() -> Option<Self> {
        let name_wide: Vec<u16> = SHARED_MEM_NAME.encode_utf16().chain(std::iter::once(0)).collect();

        let handle = unsafe {
            OpenFileMappingW(
                FILE_MAP_READ.0,
                false,
                windows::core::PCWSTR(name_wide.as_ptr()),
            )
        };

        let handle = match handle {
            Ok(h) => h,
            Err(_) => return None, // Host hasn't created shared memory yet
        };

        let ptr = unsafe {
            MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0)
        };

        if ptr.Value.is_null() {
            unsafe { let _ = CloseHandle(handle); }
            return None;
        }

        log_to_file("[ipc] shared memory opened successfully");

        Some(Self {
            handle,
            ptr: ptr.Value as *const SharedOverlayState,
            last_sequence: 0,
        })
    }

    /// Read the active slot. Returns None if data hasn't changed since last read.
    pub fn read(&mut self) -> Option<&OverlaySlot> {
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
}

impl Drop for SharedMemoryReader {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr as *mut std::ffi::c_void,
            });
            let _ = CloseHandle(self.handle);
        }
        log_to_file("[ipc] shared memory closed");
    }
}
```

- [ ] **Step 2: Add mod declaration to lib.rs**

Add `mod ipc;` after the existing module declarations in `overlay-dll/src/lib.rs`:

```rust
mod logging;
mod hook;
mod present;
mod shaders;
mod state_backup;
mod renderer;
mod ipc;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p omni-overlay-dll`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add overlay-dll/src/ipc/ overlay-dll/src/lib.rs
git commit -m "feat(overlay-dll): add shared memory reader for IPC with host"
```

---

### Task 7: D2D1/DirectWrite Renderer

**Files:**
- Rewrite: `overlay-dll/src/renderer.rs`

This replaces the entire shader-based renderer with D2D1/DirectWrite.

- [ ] **Step 1: Rewrite overlay-dll/src/renderer.rs**

```rust
use std::ffi::c_void;
use std::mem::ManuallyDrop;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D1_ALPHA_MODE_PREMULTIPLIED,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory1, ID2D1RenderTarget, ID2D1SolidColorBrush,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::core::{Interface, w};

use omni_shared::{ComputedWidget, WidgetType, read_fixed_str};

use crate::logging::log_to_file;

pub struct OverlayRenderer {
    d2d_factory: ID2D1Factory1,
    dwrite_factory: IDWriteFactory,
    render_target: Option<ID2D1RenderTarget>,
}

impl OverlayRenderer {
    /// Initialize D2D and DirectWrite factories.
    /// The render target is created lazily on first render (needs the swap chain surface).
    pub fn init() -> Result<Self, String> {
        log_to_file("[renderer] initializing D2D1 + DirectWrite");

        let d2d_factory: ID2D1Factory1 = unsafe {
            D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                .map_err(|e| format!("D2D1CreateFactory failed: {e}"))?
        };

        let dwrite_factory: IDWriteFactory = unsafe {
            DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
                .map_err(|e| format!("DWriteCreateFactory failed: {e}"))?
        };

        log_to_file("[renderer] D2D1 + DirectWrite initialized");

        Ok(Self {
            d2d_factory,
            dwrite_factory,
            render_target: None,
        })
    }

    /// Ensure we have a render target for the current swap chain back buffer.
    unsafe fn ensure_render_target(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        if self.render_target.is_some() {
            return Ok(());
        }

        let sc: IDXGISwapChain = std::mem::transmute_copy(&swap_chain_ptr);
        let sc = ManuallyDrop::new(sc);

        let back_buffer: windows::Win32::Graphics::Dxgi::IDXGISurface = sc
            .GetBuffer(0)
            .map_err(|e| format!("GetBuffer(0) failed: {e}"))?;

        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_UNKNOWN,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: windows::Win32::Graphics::Direct2D::D2D1_FEATURE_LEVEL_DEFAULT,
        };

        let rt = self.d2d_factory
            .CreateDxgiSurfaceRenderTarget(&back_buffer, &props)
            .map_err(|e| format!("CreateDxgiSurfaceRenderTarget failed: {e}"))?;

        self.render_target = Some(rt);

        log_to_file("[renderer] D2D render target created from swap chain");
        Ok(())
    }

    /// Render a list of computed widgets onto the swap chain back buffer.
    pub unsafe fn render(&mut self, swap_chain_ptr: *mut c_void, widgets: &[ComputedWidget]) {
        if let Err(e) = self.ensure_render_target(swap_chain_ptr) {
            log_to_file(&format!("[renderer] failed to create render target: {e}"));
            return;
        }

        let rt = match &self.render_target {
            Some(rt) => rt,
            None => return,
        };

        rt.BeginDraw();

        for widget in widgets {
            if widget.opacity <= 0.0 {
                continue;
            }

            let rect = D2D_RECT_F {
                left: widget.x,
                top: widget.y,
                right: widget.x + widget.width,
                bottom: widget.y + widget.height,
            };

            // Draw background
            let bg = &widget.bg_color_rgba;
            if bg[3] > 0 {
                let bg_color = D2D1_COLOR_F {
                    r: bg[0] as f32 / 255.0,
                    g: bg[1] as f32 / 255.0,
                    b: bg[2] as f32 / 255.0,
                    a: (bg[3] as f32 / 255.0) * widget.opacity,
                };

                if let Ok(brush) = rt.CreateSolidColorBrush(&bg_color, None) {
                    let radius = widget.border_radius[0]; // simplified: use top-left for all
                    if radius > 0.0 {
                        let rounded = D2D1_ROUNDED_RECT {
                            rect,
                            radiusX: radius,
                            radiusY: radius,
                        };
                        rt.FillRoundedRectangle(&rounded, &brush);
                    } else {
                        rt.FillRectangle(&rect, &brush);
                    }
                }
            }

            // Draw text
            let text = read_fixed_str(&widget.format_pattern);
            if text.is_empty() {
                continue;
            }

            let font_weight = if widget.font_weight >= 700 {
                DWRITE_FONT_WEIGHT_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            };

            let text_format = self.dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                font_weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                widget.font_size,
                w!("en-us"),
            );

            let text_format = match text_format {
                Ok(tf) => tf,
                Err(_) => continue,
            };

            let _ = text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);

            let fg = &widget.color_rgba;
            let fg_color = D2D1_COLOR_F {
                r: fg[0] as f32 / 255.0,
                g: fg[1] as f32 / 255.0,
                b: fg[2] as f32 / 255.0,
                a: (fg[3] as f32 / 255.0) * widget.opacity,
            };

            if let Ok(brush) = rt.CreateSolidColorBrush(&fg_color, None) {
                let text_wide: Vec<u16> = text.encode_utf16().collect();
                rt.DrawText(
                    &text_wide,
                    &text_format,
                    &rect,
                    &brush,
                    windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NONE,
                    windows::Win32::Graphics::DirectWrite::DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }

        let _ = rt.EndDraw(None, None);
    }

    /// Release the render target. Call before ResizeBuffers.
    pub fn release_render_target(&mut self) {
        self.render_target = None;
        log_to_file("[renderer] D2D render target released");
    }

    /// Recreate render target after ResizeBuffers.
    pub unsafe fn recreate_render_target(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        self.render_target = None;
        self.ensure_render_target(swap_chain_ptr)
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-overlay-dll`
Expected: Compiles (with existing warnings about static mut refs in present.rs — those will be updated in the next task).

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/src/renderer.rs
git commit -m "feat(overlay-dll): rewrite renderer with D2D1/DirectWrite, replacing shader pipeline"
```

---

### Task 8: Wire Present Hooks to D2D Renderer + Shared Memory

**Files:**
- Rewrite: `overlay-dll/src/present.rs`

- [ ] **Step 1: Rewrite overlay-dll/src/present.rs**

```rust
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows::core::HRESULT;

use crate::logging::log_to_file;
use crate::renderer::OverlayRenderer;
use crate::ipc::SharedMemoryReader;

pub type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT;
pub type Present1Fn = unsafe extern "system" fn(*mut c_void, u32, u32, *const c_void) -> HRESULT;
pub type ResizeBuffersFn = unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, u32) -> HRESULT;

pub static mut ORIGINAL_PRESENT: Option<PresentFn> = None;
pub static mut ORIGINAL_PRESENT1: Option<Present1Fn> = None;
pub static mut ORIGINAL_RESIZE_BUFFERS: Option<ResizeBuffersFn> = None;

static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);
static RENDERER_INIT_DONE: AtomicBool = AtomicBool::new(false);
static mut RENDERER: Option<OverlayRenderer> = None;
static mut SHM_READER: Option<SharedMemoryReader> = None;

/// Initialize the renderer on the first Present call.
unsafe fn ensure_renderer() {
    if RENDERER_INIT_DONE.load(Ordering::Acquire) {
        return;
    }

    match OverlayRenderer::init() {
        Ok(r) => {
            RENDERER = Some(r);
            RENDERER_INIT_DONE.store(true, Ordering::Release);
            log_to_file("[present] D2D renderer initialized on first frame");
        }
        Err(e) => {
            log_to_file(&format!("[present] FATAL: renderer init failed: {e}"));
            RENDERER_INIT_DONE.store(true, Ordering::Release);
        }
    }
}

/// Try to open shared memory if not already open.
unsafe fn ensure_shm_reader() {
    if SHM_READER.is_some() {
        return;
    }
    if let Some(reader) = SharedMemoryReader::open() {
        SHM_READER = Some(reader);
    }
    // If it fails, we'll try again next frame — host might not be running yet
}

/// Common rendering logic shared by hooked_present and hooked_present1.
unsafe fn render_overlay(swap_chain: *mut c_void) {
    ensure_renderer();
    ensure_shm_reader();

    let renderer = match &mut RENDERER {
        Some(r) => r,
        None => return,
    };

    // Read widgets from shared memory
    let widgets = match &mut SHM_READER {
        Some(reader) => {
            let slot = reader.read_current();
            let count = slot.widget_count as usize;
            if count > 0 {
                &slot.widgets[..count]
            } else {
                return; // No widgets to render
            }
        }
        None => return, // No shared memory — host not running
    };

    renderer.render(swap_chain, widgets);
}

/// Drop the renderer and shared memory reader. Called during shutdown.
pub unsafe fn destroy_renderer() {
    RENDERER_INIT_DONE.store(false, Ordering::SeqCst);
    if let Some(renderer) = RENDERER.take() {
        drop(renderer);
        log_to_file("[present] D2D renderer destroyed");
    }
    if let Some(reader) = SHM_READER.take() {
        drop(reader);
        log_to_file("[present] shared memory reader closed");
    }
}

pub unsafe extern "system" fn hooked_present(
    swap_chain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
    if count % 300 == 0 {
        log_to_file(&format!(
            "[present] frame {count}, sync_interval={sync_interval}, flags={flags:#010x}"
        ));
    }

    render_overlay(swap_chain);

    if let Some(original) = ORIGINAL_PRESENT {
        original(swap_chain, sync_interval, flags)
    } else {
        HRESULT(0)
    }
}

pub unsafe extern "system" fn hooked_present1(
    swap_chain: *mut c_void,
    sync_interval: u32,
    present_flags: u32,
    present_params: *const c_void,
) -> HRESULT {
    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
    if count % 300 == 0 {
        log_to_file(&format!(
            "[present1] frame {count}, sync_interval={sync_interval}, flags={present_flags:#010x}"
        ));
    }

    render_overlay(swap_chain);

    if let Some(original) = ORIGINAL_PRESENT1 {
        original(swap_chain, sync_interval, present_flags, present_params)
    } else {
        HRESULT(0)
    }
}

pub unsafe extern "system" fn hooked_resize_buffers(
    swap_chain: *mut c_void,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: u32,
    swap_chain_flags: u32,
) -> HRESULT {
    log_to_file(&format!(
        "[resize_buffers] {width}x{height}, buffers={buffer_count}"
    ));

    // Release D2D render target before resize
    if let Some(renderer) = &mut RENDERER {
        renderer.release_render_target();
    }

    // Call original ResizeBuffers
    let result = if let Some(original) = ORIGINAL_RESIZE_BUFFERS {
        original(swap_chain, buffer_count, width, height, new_format, swap_chain_flags)
    } else {
        HRESULT(0)
    };

    // Recreate render target after resize
    if result.is_ok() {
        if let Some(renderer) = &mut RENDERER {
            if let Err(e) = renderer.recreate_render_target(swap_chain) {
                log_to_file(&format!("[resize_buffers] failed to recreate render target: {e}"));
            }
        }
    }

    result
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-overlay-dll`
Expected: Compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/src/present.rs
git commit -m "feat(overlay-dll): wire present hooks to D2D renderer and shared memory reader"
```

---

### Task 9: Delete Shader Pipeline

**Files:**
- Delete: `overlay-dll/src/shaders.rs`
- Modify: `overlay-dll/src/lib.rs` (remove `mod shaders;`)

- [ ] **Step 1: Delete overlay-dll/src/shaders.rs**

```bash
rm overlay-dll/src/shaders.rs
```

- [ ] **Step 2: Remove mod shaders from lib.rs**

Remove the line `mod shaders;` from `overlay-dll/src/lib.rs`.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p omni-overlay-dll`
Expected: Compiles with no errors. If `state_backup.rs` is still referenced by the old renderer, also remove `mod state_backup;` and delete the file if nothing else uses it.

- [ ] **Step 4: Commit**

```bash
git add -u overlay-dll/src/
git commit -m "refactor(overlay-dll): remove shader pipeline, replaced by D2D1"
```

---

### Task 10: Integration Test — Live CPU Usage in Game

This is a manual integration test verifying the full pipeline.

- [ ] **Step 1: Build everything**

```bash
cargo build -p omni-host && cargo build -p omni-overlay-dll
```

- [ ] **Step 2: Start watch mode**

```bash
cargo run -p omni-host -- --watch target/debug/omni_overlay_dll.dll
```

Expected output:
```
INFO omni_host::ipc: Shared memory created size=... name="OmniOverlay_SharedState"
INFO omni_host::sensors::cpu: sysinfo: CPU sensor initialized core_count=...
INFO omni_host: Omni host starting in watch mode ...
```

- [ ] **Step 3: Launch a DX11 game**

Launch any DX11 game. Within a few seconds:
- Scanner detects and injects the DLL
- DLL log (`%TEMP%\omni_overlay.log`) shows renderer init and shared memory open
- **Live CPU usage text appears in the game** (top-left, white text on dark background)
- Text updates as CPU usage changes

- [ ] **Step 4: Test host restart resilience**

Press Ctrl+C to stop the host. Verify:
- Overlay disappears (DLL ejected, or renders nothing with no shared memory)
- Game continues running without crash

Restart the host. Verify:
- Overlay reappears with live CPU data
- Game remains stable

- [ ] **Step 5: Test Task Manager kill resilience**

Kill the host process via Task Manager. Verify:
- Game continues running (DLL still loaded but no new data)
- Overlay may show stale data or disappear (acceptable for this phase)

Restart the host. Verify:
- Host reconnects to existing DLL
- Overlay resumes with live CPU data

- [ ] **Step 6: Verify no GPU/RAM/Frame data**

Check the overlay — only CPU data should be displayed. GPU, RAM, and frame timing should not appear (those sensors aren't implemented yet).

- [ ] **Step 7: Troubleshooting**

If the overlay doesn't appear:
- Check `%TEMP%\omni_overlay.log` for errors
- Set `RUST_LOG=debug` for host-side diagnostics
- Verify shared memory was created: the host log should show the `Shared memory created` line
- If D2D render target creation fails, check the DLL log for the specific error

- [ ] **Step 8: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 5 integration test"
```

---

## Phase 5 Complete — Summary

At this point you have:

1. **Shared memory IPC** — Host creates `CreateFileMappingW`-backed named shared memory with lock-free double buffer. DLL reads lazily.
2. **CPU sensor polling** — Background thread polls `sysinfo` at 1s intervals, writes to shared memory via channel.
3. **D2D1/DirectWrite renderer** — Replaces the shader-based test quad. Draws widgets (background rects + text) from shared memory data.
4. **End-to-end pipeline** — Real CPU usage flows: `sysinfo` → `SensorSnapshot` → `ComputedWidget` → shared memory → DLL → D2D → game back buffer.
5. **Phase 4 resilience preserved** — Host start/stop/crash/restart all work without crashing the game.

**Next:** Phase 6 adds DX12 support via D3D11On12 compatibility layer.
