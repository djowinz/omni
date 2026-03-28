# Phase 1: Workspace + Shared Types + DLL Injection PoC

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Set up the Cargo workspace with three crates (shared, host, overlay-dll), define the core `#[repr(C)]` shared types, build a minimal DLL that logs on attach, and build the host-side injector that loads the DLL into a target game process.

**Architecture:** Three-crate workspace. `shared` defines `#[repr(C)]` types for cross-process communication. `overlay-dll` is a `cdylib` with a `DllMain` that writes to a log file on attach/detach. `host` uses Win32 APIs (`OpenProcess`, `VirtualAllocEx`, `WriteProcessMemory`, `CreateRemoteThread` + `LoadLibraryW`) to inject the DLL into a target process by PID.

**Tech Stack:** Rust, `windows` crate (windows-rs) for Win32 APIs, `tracing` + `tracing-subscriber` for host logging.

**Testing notes:** This phase involves Windows-specific DLL injection — unit tests cover shared types and serialization. Integration testing is manual: inject into a running game, check that the DLL's log file appears. Each task notes what to verify and how.

---

## File Map

```
repo/
  Cargo.toml                        # Workspace root
  .gitignore                         # Rust/Windows ignores

  shared/
    Cargo.toml                       # No external dependencies
    src/
      lib.rs                         # Re-exports
      sensor_types.rs                # SensorSnapshot, CpuData, GpuData, RamData, FrameData
      widget_types.rs                # ComputedWidget, WidgetType, SensorSource, GradientDef, ShadowDef
      ipc_protocol.rs                # SharedOverlayState, OverlaySlot, constants

  overlay-dll/
    Cargo.toml                       # cdylib, depends on shared + windows
    src/
      lib.rs                         # DllMain entry, log-on-attach

  host/
    Cargo.toml                       # depends on shared + windows + tracing
    src/
      main.rs                        # CLI entry: parse PID arg, inject
      injector/
        mod.rs                       # inject_dll(pid, dll_path) function
```

---

### Task 1: Workspace Root + .gitignore

**Files:**
- Create: `Cargo.toml`
- Create: `.gitignore`

- [ ] **Step 1: Create workspace Cargo.toml**

```toml
[workspace]
members = ["shared", "host", "overlay-dll"]
resolver = "2"
```

- [ ] **Step 2: Create .gitignore**

```gitignore
/target
*.pdb
*.dll
*.exe
*.log
```

- [ ] **Step 3: Verify workspace file exists**

Run: `cat Cargo.toml`
Expected: Shows workspace definition with three members.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml .gitignore
git commit -m "feat: initialize cargo workspace with three crate members"
```

---

### Task 2: Shared Crate — Sensor Types

**Files:**
- Create: `shared/Cargo.toml`
- Create: `shared/src/lib.rs`
- Create: `shared/src/sensor_types.rs`

- [ ] **Step 1: Create shared/Cargo.toml**

```toml
[package]
name = "omni-shared"
version = "0.1.0"
edition = "2021"

[dependencies]
# No external dependencies — pure #[repr(C)] types
```

- [ ] **Step 2: Create shared/src/sensor_types.rs**

```rust
/// Sensor data types shared between host and overlay DLL.
/// All structs are #[repr(C)] because they cross process boundaries via shared memory.

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SensorSnapshot {
    pub timestamp_ms: u64,
    pub cpu: CpuData,
    pub gpu: GpuData,
    pub ram: RamData,
    pub frame: FrameData,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CpuData {
    /// Overall CPU usage as a percentage (0.0–100.0).
    pub total_usage_percent: f32,
    /// Per-core usage percentages. Unused cores are set to -1.0.
    pub per_core_usage: [f32; 32],
    pub core_count: u32,
    /// Per-core frequency in MHz. Unused cores are 0.
    pub per_core_freq_mhz: [u32; 32],
    /// CPU package temperature in Celsius. f32::NAN if unavailable.
    pub package_temp_c: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GpuData {
    pub usage_percent: f32,
    pub temp_c: f32,
    pub core_clock_mhz: u32,
    pub mem_clock_mhz: u32,
    pub vram_used_mb: u32,
    pub vram_total_mb: u32,
    pub fan_speed_rpm: u32,
    pub power_draw_w: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RamData {
    pub usage_percent: f32,
    pub used_mb: u64,
    pub total_mb: u64,
    pub frequency_mhz: u32,
    pub timing_cl: u32,
    /// RAM temperature in Celsius. f32::NAN if unavailable.
    pub temp_c: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct FrameData {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub frame_time_avg_ms: f32,
    pub frame_time_1percent_ms: f32,
    pub frame_time_01percent_ms: f32,
    /// false if no frame data source is active.
    pub available: bool,
}

impl Default for SensorSnapshot {
    fn default() -> Self {
        Self {
            timestamp_ms: 0,
            cpu: CpuData::default(),
            gpu: GpuData::default(),
            ram: RamData::default(),
            frame: FrameData::default(),
        }
    }
}

impl Default for CpuData {
    fn default() -> Self {
        Self {
            total_usage_percent: 0.0,
            per_core_usage: [-1.0; 32],
            core_count: 0,
            per_core_freq_mhz: [0; 32],
            package_temp_c: f32::NAN,
        }
    }
}

impl Default for GpuData {
    fn default() -> Self {
        Self {
            usage_percent: 0.0,
            temp_c: f32::NAN,
            core_clock_mhz: 0,
            mem_clock_mhz: 0,
            vram_used_mb: 0,
            vram_total_mb: 0,
            fan_speed_rpm: 0,
            power_draw_w: 0.0,
        }
    }
}

impl Default for RamData {
    fn default() -> Self {
        Self {
            usage_percent: 0.0,
            used_mb: 0,
            total_mb: 0,
            frequency_mhz: 0,
            timing_cl: 0,
            temp_c: f32::NAN,
        }
    }
}

impl Default for FrameData {
    fn default() -> Self {
        Self {
            fps: 0.0,
            frame_time_ms: 0.0,
            frame_time_avg_ms: 0.0,
            frame_time_1percent_ms: 0.0,
            frame_time_01percent_ms: 0.0,
            available: false,
        }
    }
}
```

- [ ] **Step 3: Create shared/src/lib.rs with sensor_types module**

```rust
pub mod sensor_types;
pub mod widget_types;
pub mod ipc_protocol;

pub use sensor_types::*;
pub use widget_types::*;
pub use ipc_protocol::*;
```

Note: `widget_types` and `ipc_protocol` modules are created in Tasks 3 and 4. For now, comment out or leave — we'll add them sequentially. Temporarily use:

```rust
pub mod sensor_types;

pub use sensor_types::*;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p omni-shared`
Expected: Compiles with no errors.

- [ ] **Step 5: Write tests for sensor types**

Add to the bottom of `shared/src/sensor_types.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn sensor_snapshot_is_repr_c_sized() {
        // Ensure the struct has a stable, non-zero size (sanity check for shared memory)
        let size = mem::size_of::<SensorSnapshot>();
        assert!(size > 0);
        // Alignment must be suitable for atomic access
        let align = mem::align_of::<SensorSnapshot>();
        assert!(align >= 4);
    }

    #[test]
    fn cpu_data_default_marks_unused_cores() {
        let cpu = CpuData::default();
        assert_eq!(cpu.core_count, 0);
        for usage in cpu.per_core_usage.iter() {
            assert_eq!(*usage, -1.0);
        }
    }

    #[test]
    fn unavailable_temps_are_nan() {
        let cpu = CpuData::default();
        assert!(cpu.package_temp_c.is_nan());

        let gpu = GpuData::default();
        assert!(gpu.temp_c.is_nan());

        let ram = RamData::default();
        assert!(ram.temp_c.is_nan());
    }

    #[test]
    fn frame_data_default_not_available() {
        let frame = FrameData::default();
        assert!(!frame.available);
    }
}
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p omni-shared`
Expected: 4 tests pass.

- [ ] **Step 7: Commit**

```bash
git add shared/
git commit -m "feat(shared): add sensor data types with repr(C) layout and defaults"
```

---

### Task 3: Shared Crate — Widget Types

**Files:**
- Create: `shared/src/widget_types.rs`
- Modify: `shared/src/lib.rs`

- [ ] **Step 1: Create shared/src/widget_types.rs**

```rust
/// Widget types shared between host and overlay DLL.
/// All types are #[repr(C)] for shared memory safety.

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ComputedWidget {
    pub widget_type: WidgetType,
    pub source: SensorSource,
    /// Absolute screen position in pixels.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub font_weight: u16,
    pub color_rgba: [u8; 4],
    pub bg_color_rgba: [u8; 4],
    pub bg_gradient: GradientDef,
    pub border_color_rgba: [u8; 4],
    pub border_width: f32,
    /// Per-corner border radius: [top-left, top-right, bottom-right, bottom-left].
    pub border_radius: [f32; 4],
    pub opacity: f32,
    pub box_shadow: ShadowDef,
    /// Format pattern, e.g. "{value:.0f}°C". Null-terminated UTF-8.
    pub format_pattern: [u8; 128],
    /// Pre-formatted label text, e.g. "CPU". Null-terminated UTF-8.
    pub label_text: [u8; 64],
    /// Sensor value above this triggers critical state. f32::NAN to disable.
    pub critical_above: f32,
    pub critical_color_rgba: [u8; 4],
    /// For graph widgets: how many seconds of history to display.
    pub history_seconds: u32,
    /// For graph widgets: data point interval in milliseconds.
    pub history_interval_ms: u32,
    pub adaptive_color: AdaptiveColorMode,
    pub adaptive_light_rgba: [u8; 4],
    pub adaptive_dark_rgba: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetType {
    Label,
    SensorValue,
    Graph,
    Bar,
    Spacer,
    Group,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorSource {
    None,
    CpuUsage,
    CpuTemp,
    CpuFreqCore0,
    CpuFreqCore1,
    CpuFreqCore2,
    CpuFreqCore3,
    CpuFreqCore4,
    CpuFreqCore5,
    CpuFreqCore6,
    CpuFreqCore7,
    CpuFreqCore8,
    CpuFreqCore9,
    CpuFreqCore10,
    CpuFreqCore11,
    CpuFreqCore12,
    CpuFreqCore13,
    CpuFreqCore14,
    CpuFreqCore15,
    CpuFreqCore16,
    CpuFreqCore17,
    CpuFreqCore18,
    CpuFreqCore19,
    CpuFreqCore20,
    CpuFreqCore21,
    CpuFreqCore22,
    CpuFreqCore23,
    CpuFreqCore24,
    CpuFreqCore25,
    CpuFreqCore26,
    CpuFreqCore27,
    CpuFreqCore28,
    CpuFreqCore29,
    CpuFreqCore30,
    CpuFreqCore31,
    GpuUsage,
    GpuTemp,
    GpuClock,
    GpuMemClock,
    GpuVram,
    GpuPower,
    GpuFan,
    RamUsage,
    RamTemp,
    RamFreq,
    Fps,
    FrameTime,
    FrameTimeAvg,
    FrameTime1Pct,
    FrameTime01Pct,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdaptiveColorMode {
    Off,
    Auto,
    Custom,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GradientDef {
    pub enabled: bool,
    pub angle_deg: f32,
    pub start_rgba: [u8; 4],
    pub end_rgba: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ShadowDef {
    pub enabled: bool,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub color_rgba: [u8; 4],
}

impl Default for ComputedWidget {
    fn default() -> Self {
        Self {
            widget_type: WidgetType::Label,
            source: SensorSource::None,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            font_size: 13.0,
            font_weight: 400,
            color_rgba: [204, 204, 204, 255], // #cccccc
            bg_color_rgba: [0, 0, 0, 0],       // transparent
            bg_gradient: GradientDef::default(),
            border_color_rgba: [0, 0, 0, 0],
            border_width: 0.0,
            border_radius: [0.0; 4],
            opacity: 1.0,
            box_shadow: ShadowDef::default(),
            format_pattern: [0; 128],
            label_text: [0; 64],
            critical_above: f32::NAN,
            critical_color_rgba: [255, 68, 68, 255], // #ff4444
            history_seconds: 0,
            history_interval_ms: 0,
            adaptive_color: AdaptiveColorMode::Off,
            adaptive_light_rgba: [255, 255, 255, 255],
            adaptive_dark_rgba: [0, 0, 0, 255],
        }
    }
}

impl Default for GradientDef {
    fn default() -> Self {
        Self {
            enabled: false,
            angle_deg: 0.0,
            start_rgba: [0; 4],
            end_rgba: [0; 4],
        }
    }
}

impl Default for ShadowDef {
    fn default() -> Self {
        Self {
            enabled: false,
            offset_x: 0.0,
            offset_y: 0.0,
            blur_radius: 0.0,
            color_rgba: [0; 4],
        }
    }
}

/// Helper to write a string into a fixed-size null-terminated byte array.
/// Truncates if the string is too long.
pub fn write_fixed_str(dest: &mut [u8], src: &str) {
    let bytes = src.as_bytes();
    let max_len = dest.len() - 1; // reserve last byte for null terminator
    let copy_len = bytes.len().min(max_len);
    dest[..copy_len].copy_from_slice(&bytes[..copy_len]);
    dest[copy_len] = 0;
    // Zero the rest
    for byte in &mut dest[copy_len + 1..] {
        *byte = 0;
    }
}

/// Helper to read a null-terminated UTF-8 string from a fixed-size byte array.
pub fn read_fixed_str(src: &[u8]) -> &str {
    let end = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    std::str::from_utf8(&src[..end]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn computed_widget_is_repr_c_sized() {
        let size = mem::size_of::<ComputedWidget>();
        assert!(size > 0);
    }

    #[test]
    fn widget_default_has_sane_values() {
        let w = ComputedWidget::default();
        assert_eq!(w.widget_type, WidgetType::Label);
        assert_eq!(w.source, SensorSource::None);
        assert_eq!(w.opacity, 1.0);
        assert_eq!(w.font_size, 13.0);
        assert!(w.critical_above.is_nan());
    }

    #[test]
    fn write_and_read_fixed_str() {
        let mut buf = [0u8; 64];
        write_fixed_str(&mut buf, "CPU Temp");
        assert_eq!(read_fixed_str(&buf), "CPU Temp");
    }

    #[test]
    fn write_fixed_str_truncates_long_input() {
        let mut buf = [0u8; 8];
        write_fixed_str(&mut buf, "This is a very long string");
        let result = read_fixed_str(&buf);
        assert_eq!(result.len(), 7); // 8 bytes - 1 null terminator
        assert_eq!(result, "This is");
    }

    #[test]
    fn sensor_source_all_cores_exist() {
        // Ensure we have all 32 core freq variants
        let _ = SensorSource::CpuFreqCore0;
        let _ = SensorSource::CpuFreqCore31;
    }
}
```

- [ ] **Step 2: Update shared/src/lib.rs to include widget_types**

```rust
pub mod sensor_types;
pub mod widget_types;

pub use sensor_types::*;
pub use widget_types::*;
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p omni-shared`
Expected: All 9 tests pass (4 sensor + 5 widget).

- [ ] **Step 4: Commit**

```bash
git add shared/src/widget_types.rs shared/src/lib.rs
git commit -m "feat(shared): add widget types, sensor source enum, and fixed-string helpers"
```

---

### Task 4: Shared Crate — IPC Protocol Types

**Files:**
- Create: `shared/src/ipc_protocol.rs`
- Modify: `shared/src/lib.rs`

- [ ] **Step 1: Create shared/src/ipc_protocol.rs**

```rust
/// IPC protocol types for shared memory between host and overlay DLL.
/// Uses a lock-free double buffer: host writes to inactive slot,
/// atomically flips active_slot, DLL reads from active slot.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::sensor_types::SensorSnapshot;
use crate::widget_types::ComputedWidget;

pub const SHARED_MEM_NAME: &str = "OmniOverlay_SharedState";
pub const CONTROL_PIPE_NAME: &str = r"\\.\pipe\OmniOverlay_Control";
pub const MAX_WIDGETS: usize = 64;

#[repr(C)]
pub struct SharedOverlayState {
    /// 0 or 1 — which slot the DLL should read from.
    pub active_slot: AtomicU64,
    pub slots: [OverlaySlot; 2],
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
        self.active_slot.load(Ordering::Acquire) as usize & 1
    }

    /// Returns the index (0 or 1) of the slot the host should write to.
    pub fn writer_slot_index(&self) -> usize {
        // Writer uses the opposite slot from the reader
        (self.active_slot.load(Ordering::Acquire) as usize & 1) ^ 1
    }

    /// Host calls this after writing to the writer slot to make it active.
    pub fn flip_slot(&self) {
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
        assert!(size > 1000, "SharedOverlayState is unexpectedly small: {size}");
    }

    #[test]
    fn slot_flip_toggles_between_0_and_1() {
        let state = SharedOverlayState {
            active_slot: AtomicU64::new(0),
            slots: [OverlaySlot::default(), OverlaySlot::default()],
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
```

- [ ] **Step 2: Update shared/src/lib.rs to include ipc_protocol**

```rust
pub mod sensor_types;
pub mod widget_types;
pub mod ipc_protocol;

pub use sensor_types::*;
pub use widget_types::*;
pub use ipc_protocol::*;
```

- [ ] **Step 3: Run all shared tests**

Run: `cargo test -p omni-shared`
Expected: All 12 tests pass (4 sensor + 5 widget + 3 ipc).

- [ ] **Step 4: Commit**

```bash
git add shared/src/ipc_protocol.rs shared/src/lib.rs
git commit -m "feat(shared): add IPC protocol with double-buffered shared memory and control messages"
```

---

### Task 5: Overlay DLL — Minimal DllMain with Logging

**Files:**
- Create: `overlay-dll/Cargo.toml`
- Create: `overlay-dll/src/lib.rs`

- [ ] **Step 1: Create overlay-dll/Cargo.toml**

```toml
[package]
name = "omni-overlay-dll"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
omni-shared = { path = "../shared" }

[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_System_SystemServices",
    "Win32_System_LibraryLoader",
]
```

Note: The `windows` crate version should match across all crates. Check the latest version at build time — use `cargo search windows` or check crates.io. As of 2025, 0.58+ is current. The features listed provide `DLL_PROCESS_ATTACH`, `DLL_PROCESS_DETACH`, `BOOL`, `TRUE`, and `HINSTANCE`.

- [ ] **Step 2: Create overlay-dll/src/lib.rs**

```rust
use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use windows::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

/// Log a message to a file next to the DLL. This is intentionally simple —
/// no dependencies beyond std. In later phases, we'll replace this with
/// proper structured logging.
fn log_to_file(msg: &str) {
    let path = std::env::temp_dir().join("omni_overlay.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "[{timestamp}] {msg}");
    }
}

/// DLL entry point. Called by Windows when the DLL is loaded/unloaded.
///
/// # Safety
/// This is called by the Windows loader. We must not do anything complex here —
/// no heap allocations beyond simple logging, no thread creation, no LoadLibrary calls.
/// See: https://learn.microsoft.com/en-us/windows/win32/dlls/dllmain
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        x if x == DLL_PROCESS_ATTACH => {
            log_to_file("omni overlay DLL attached to process");
        }
        x if x == DLL_PROCESS_DETACH => {
            log_to_file("omni overlay DLL detached from process");
        }
        _ => {}
    }
    TRUE
}
```

- [ ] **Step 3: Verify it compiles to a DLL**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles successfully. Check the output:

Run: `ls target/debug/omni_overlay_dll.dll 2>/dev/null || ls target/debug/libomni_overlay_dll.dll 2>/dev/null || echo "Check target/debug/ for the .dll file"`
Expected: A `.dll` file exists in `target/debug/`.

Note: On Windows, `cdylib` crates produce `.dll` files. The exact name may be `omni_overlay_dll.dll`. If building from WSL targeting Windows, you'll need to cross-compile — see Step 4.

- [ ] **Step 4: Set up Windows cross-compilation (if building from WSL)**

If you're building from WSL, you need the Windows target:

Run: `rustup target add x86_64-pc-windows-msvc`

Then build with:

Run: `cargo build -p omni-overlay-dll --target x86_64-pc-windows-msvc`

Alternatively, build directly from a Windows terminal (PowerShell/cmd) where the MSVC toolchain is available. The DLL must be a native Windows binary — it will be loaded into a Windows game process.

For the rest of this plan, all `overlay-dll` and `host` builds should target `x86_64-pc-windows-msvc`.

- [ ] **Step 5: Commit**

```bash
git add overlay-dll/
git commit -m "feat(overlay-dll): minimal DllMain that logs attach/detach to temp file"
```

---

### Task 6: Host — Cargo Setup + Tracing

**Files:**
- Create: `host/Cargo.toml`
- Create: `host/src/main.rs`

- [ ] **Step 1: Create host/Cargo.toml**

```toml
[package]
name = "omni-host"
version = "0.1.0"
edition = "2021"

[dependencies]
omni-shared = { path = "../shared" }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_System_Threading",
    "Win32_System_Memory",
    "Win32_System_Diagnostics_Debug",
    "Win32_System_LibraryLoader",
    "Win32_Security",
]
```

- [ ] **Step 2: Create host/src/main.rs (placeholder with tracing)**

```rust
use tracing::{info, error};
use tracing_subscriber::EnvFilter;

mod injector;

fn main() {
    // Initialize tracing with file + console output
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: omni-host <PID> <DLL_PATH>");
        eprintln!("  PID       - Process ID of the target game");
        eprintln!("  DLL_PATH  - Absolute path to omni_overlay_dll.dll");
        std::process::exit(1);
    }

    let pid: u32 = args[1].parse().unwrap_or_else(|_| {
        error!("Invalid PID: {}", args[1]);
        std::process::exit(1);
    });

    let dll_path = &args[2];

    if !std::path::Path::new(dll_path).exists() {
        error!(dll_path, "DLL file not found");
        std::process::exit(1);
    }

    info!(pid, dll_path, "Omni host starting — injecting overlay DLL");

    match injector::inject_dll(pid, dll_path) {
        Ok(()) => info!(pid, "DLL injection successful"),
        Err(e) => {
            error!(pid, error = %e, "DLL injection failed");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 3: Create placeholder injector module**

Create `host/src/injector/mod.rs`:

```rust
/// DLL injection via CreateRemoteThread + LoadLibraryW.
/// Injects a DLL into a target process by PID.

pub fn inject_dll(_pid: u32, _dll_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    todo!("DLL injection implementation in next task")
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p omni-host`
Expected: Compiles successfully (the `todo!()` is fine — it only panics at runtime).

- [ ] **Step 5: Commit**

```bash
git add host/
git commit -m "feat(host): add CLI entry point with tracing and placeholder injector module"
```

---

### Task 7: Host — DLL Injector Implementation

**Files:**
- Modify: `host/src/injector/mod.rs`

This is the core Win32 injection logic. The sequence is:
1. `OpenProcess` — get a handle to the target process
2. `VirtualAllocEx` — allocate memory in the target process for the DLL path string
3. `WriteProcessMemory` — write the DLL path (as wide UTF-16) into that memory
4. `GetProcAddress(GetModuleHandleW("kernel32"), "LoadLibraryW")` — find LoadLibraryW address
5. `CreateRemoteThread` — call LoadLibraryW in the target process with our DLL path

- [ ] **Step 1: Implement inject_dll**

Replace `host/src/injector/mod.rs` with:

```rust
/// DLL injection via CreateRemoteThread + LoadLibraryW.
///
/// This module opens the target process, allocates memory for the DLL path,
/// writes the path as UTF-16, and creates a remote thread that calls LoadLibraryW.
///
/// Requires the host process to have sufficient privileges (usually admin or
/// same-user) to open the target process with the necessary access rights.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;

use tracing::{debug, info};
use windows::Win32::Foundation::{CloseHandle, HANDLE, BOOL};
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, WaitForSingleObject, PROCESS_ALL_ACCESS,
};
use windows::core::{s, w, PCSTR};

/// Inject a DLL into a target process.
///
/// # Arguments
/// * `pid` - Process ID of the target game
/// * `dll_path` - Absolute path to the DLL file on disk
///
/// # Errors
/// Returns an error if any Win32 API call fails (insufficient privileges,
/// invalid PID, etc.)
pub fn inject_dll(pid: u32, dll_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Convert DLL path to wide string (UTF-16) with null terminator
    let wide_path: Vec<u16> = OsStr::new(dll_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path_byte_size = wide_path.len() * std::mem::size_of::<u16>();

    debug!(pid, dll_path, path_byte_size, "Opening target process");

    // Step 1: Open the target process
    let process_handle: HANDLE = unsafe { OpenProcess(PROCESS_ALL_ACCESS, false, pid)? };

    // Wrap in a guard to ensure we always close the handle
    let result = do_injection(process_handle, &wide_path, path_byte_size);

    unsafe {
        let _ = CloseHandle(process_handle);
    }

    result
}

fn do_injection(
    process: HANDLE,
    wide_path: &[u16],
    path_byte_size: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // Step 2: Allocate memory in the target process for the DLL path
    let remote_mem = unsafe {
        VirtualAllocEx(
            process,
            Some(ptr::null()),
            path_byte_size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };

    if remote_mem.is_null() {
        return Err("VirtualAllocEx failed — could not allocate memory in target process".into());
    }

    debug!(?remote_mem, "Allocated memory in target process");

    // Step 3: Write the DLL path into the allocated memory
    let write_result = unsafe {
        WriteProcessMemory(
            process,
            remote_mem,
            wide_path.as_ptr() as *const _,
            path_byte_size,
            None,
        )
    };

    if write_result.is_err() {
        unsafe {
            VirtualFreeEx(process, remote_mem, 0, MEM_RELEASE);
        }
        return Err(format!("WriteProcessMemory failed: {:?}", write_result.err()).into());
    }

    debug!("Wrote DLL path to target process memory");

    // Step 4: Get the address of LoadLibraryW in kernel32.dll
    // kernel32.dll is loaded at the same base address in every process (ASLR applies
    // per-boot, but within a boot session, the address is the same across processes).
    let kernel32 = unsafe { GetModuleHandleW(w!("kernel32.dll"))? };
    let load_library_addr = unsafe { GetProcAddress(kernel32, s!("LoadLibraryW")) };

    let load_library_addr = match load_library_addr {
        Some(addr) => addr,
        None => {
            unsafe {
                VirtualFreeEx(process, remote_mem, 0, MEM_RELEASE);
            }
            return Err("GetProcAddress failed — could not find LoadLibraryW".into());
        }
    };

    debug!(?load_library_addr, "Found LoadLibraryW address");

    // Step 5: Create a remote thread that calls LoadLibraryW(our_dll_path)
    // SAFETY: load_library_addr is a valid LPTHREAD_START_ROUTINE — LoadLibraryW
    // takes a single LPCWSTR parameter and returns HMODULE (a pointer-sized value).
    let thread_handle = unsafe {
        CreateRemoteThread(
            process,
            None,                                               // default security
            0,                                                  // default stack size
            Some(std::mem::transmute(load_library_addr)),        // LoadLibraryW
            Some(remote_mem),                                   // DLL path as parameter
            0,                                                  // run immediately
            None,                                               // don't need thread ID
        )?
    };

    info!("Created remote thread — waiting for DLL to load");

    // Wait for the remote thread to finish (LoadLibraryW returns)
    unsafe {
        WaitForSingleObject(thread_handle, 10_000); // 10 second timeout
        let _ = CloseHandle(thread_handle);
    }

    // Clean up the allocated memory (LoadLibraryW has already copied the path)
    unsafe {
        VirtualFreeEx(process, remote_mem, 0, MEM_RELEASE);
    }

    info!("DLL injection complete");
    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p omni-host --target x86_64-pc-windows-msvc`
Expected: Compiles with no errors. There may be warnings about unused imports that the compiler will note — address if present.

Note: If building from WSL without cross-compilation set up, build from a Windows terminal instead. The `windows` crate features require the Windows SDK headers.

- [ ] **Step 3: Commit**

```bash
git add host/src/injector/mod.rs
git commit -m "feat(host): implement DLL injection via CreateRemoteThread + LoadLibraryW"
```

---

### Task 8: Integration Test — Inject into a Real Game

This is a manual integration test. No code to write — we're validating the full pipeline.

- [ ] **Step 1: Build both binaries (release mode for the DLL)**

Run from a Windows terminal:
```powershell
cargo build -p omni-overlay-dll --release
cargo build -p omni-host
```

Note the DLL path. It will be something like:
`C:\Users\DyllenOwens\Projects\omni\target\release\omni_overlay_dll.dll`

- [ ] **Step 2: Launch a game and find its PID**

Open a game (any DX11 or DX12 title). Open Task Manager → Details tab, find the game's process, note the PID.

Alternatively, from PowerShell:
```powershell
Get-Process | Where-Object { $_.MainWindowTitle -ne "" } | Select-Object Id, ProcessName, MainWindowTitle
```

- [ ] **Step 3: Run the injector**

From a Windows terminal (run as Administrator if the game is elevated):
```powershell
cargo run -p omni-host -- <GAME_PID> "C:\Users\DyllenOwens\Projects\omni\target\release\omni_overlay_dll.dll"
```

Expected console output:
```
INFO omni_host: Omni host starting — injecting overlay DLL pid=<PID> dll_path="..."
INFO omni_host::injector: Created remote thread — waiting for DLL to load
INFO omni_host::injector: DLL injection complete
INFO omni_host: DLL injection successful pid=<PID>
```

- [ ] **Step 4: Check the log file**

Open the log file written by the DLL:
```powershell
Get-Content $env:TEMP\omni_overlay.log
```

Expected:
```
[<timestamp>] omni overlay DLL attached to process
```

- [ ] **Step 5: Close the game and check detach log**

After closing the game, check the log again:
```powershell
Get-Content $env:TEMP\omni_overlay.log
```

Expected — a new line:
```
[<timestamp>] omni overlay DLL detached from process
```

- [ ] **Step 6: Troubleshooting**

If injection fails:
- **"Access denied"**: Run the host as Administrator
- **No log file**: The DLL may have failed to load. Check that the DLL path is absolute and correct. Try injecting into a simpler process first (e.g., `notepad.exe`) to isolate whether it's a permissions issue or a game-specific issue
- **Game crashes**: Check that you built the DLL for the correct architecture (x64). Most modern games are 64-bit — the DLL must match

- [ ] **Step 7: Commit any fixes discovered during testing**

If you needed to change any code to make injection work:
```bash
git add -A
git commit -m "fix: address issues found during injection integration test"
```

---

## Phase 1 Complete — Summary

At this point you have:

1. A Cargo workspace with three crates (`shared`, `host`, `overlay-dll`)
2. All `#[repr(C)]` shared types defined and tested (sensor data, widget types, IPC protocol)
3. A minimal DLL that confirms it loaded via a log file
4. A host binary that injects the DLL into any process by PID
5. Verified end-to-end: DLL loads in a real game process

**Next:** Phase 2 will hook `IDXGISwapChain::Present` inside the DLL to intercept every frame the game renders.
