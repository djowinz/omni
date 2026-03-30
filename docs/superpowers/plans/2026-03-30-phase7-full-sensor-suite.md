# Phase 7: Full Sensor Suite

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand sensor polling to include GPU data (NVAPI FFI), RAM usage (sysinfo), and CPU temperature (WMI), displaying a full hardware dashboard in the overlay.

**Architecture:** Four sensor modules feed into the existing `SensorPoller` background thread. GPU data comes from thin FFI bindings to `nvapi64.dll` (resolved via `nvapi_QueryInterface`). CPU temperature comes from WMI `MSAcpi_ThermalZoneTemperature`. RAM comes from the existing `sysinfo::System`. A `WidgetBuilder` abstraction replaces the hardcoded single-widget function, producing a `Vec<ComputedWidget>` ready for Phase 9a drop-in replacement.

**Tech Stack:** Rust, `nvapi64.dll` FFI (NVIDIA GPU), `wmi` crate (CPU temp), `sysinfo` (CPU/RAM), `windows` crate (WMI COM features).

**Testing notes:** NVAPI resolution is testable if nvapi64.dll is present (NVIDIA system). WMI temperature conversion has a pure unit test. RAM polling is testable against the real system. Widget builder output is fully unit-testable. Full pipeline tested manually in-game.

**Depends on:** Phases 1–6 complete (injection, hooks, D2D renderer, shared memory, DX11+DX12 support).

---

## File Map

```
host/
  Cargo.toml                         # Add wmi crate
  src/
    main.rs                          # Replace build_cpu_widget with WidgetBuilder
    sensors/
      mod.rs                         # Integrate all sensors into SensorPoller
      cpu.rs                         # Refactor: share sysinfo::System, add RAM data
      cpu_temp.rs                    # WMI MSAcpi_ThermalZoneTemperature
      gpu.rs                         # NVAPI FFI bindings + GpuPoller
      ram.rs                         # RAM via shared sysinfo::System
    widget_builder.rs                # WidgetBuilder: snapshot → Vec<ComputedWidget>
```

---

### Task 1: Add WMI Dependency

**Files:**
- Modify: `host/Cargo.toml`

- [ ] **Step 1: Add wmi crate to dependencies**

Add `wmi = "0.14"` to the `[dependencies]` section:

```toml
[dependencies]
omni-shared = { path = "../shared" }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ctrlc = "3"
sysinfo = "0.35"
wmi = "0.14"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Downloads `wmi` and its COM dependencies, compiles.

- [ ] **Step 3: Commit**

```bash
git add host/Cargo.toml Cargo.lock
git commit -m "feat(host): add wmi crate for CPU temperature queries"
```

---

### Task 2: NVAPI FFI Bindings — GPU Sensor

**Files:**
- Create: `host/src/sensors/gpu.rs`

This is the most complex sensor module. It loads `nvapi64.dll` at runtime, resolves function pointers via `nvapi_QueryInterface`, and polls GPU data.

- [ ] **Step 1: Create host/src/sensors/gpu.rs**

```rust
//! NVIDIA GPU sensor via NVAPI FFI bindings.
//!
//! NVAPI uses a single exported function `nvapi_QueryInterface(id) -> fn_ptr`
//! to resolve all API functions by their 32-bit ID. This module loads the DLL
//! at runtime and resolves only the functions we need.

use std::ffi::c_void;
use std::mem;

use omni_shared::GpuData;
use tracing::{info, warn, debug};
use windows::Win32::System::LibraryLoader::{LoadLibraryA, GetProcAddress};
use windows::core::s;

// ─── NVAPI function IDs ──────────────────────────────────────────────────────

const NVAPI_INITIALIZE: u32 = 0x0150E828;
const NVAPI_ENUM_PHYSICAL_GPUS: u32 = 0xE5AC921F;
const NVAPI_GPU_GET_USAGES: u32 = 0x189A1FDF;
const NVAPI_GPU_GET_THERMAL_SETTINGS: u32 = 0xE3640A56;
const NVAPI_GPU_GET_ALL_CLOCK_FREQUENCIES: u32 = 0xDCB616C3;
const NVAPI_GPU_GET_MEMORY_INFO_EX: u32 = 0xC0599498;
const NVAPI_GPU_GET_TACH_READING: u32 = 0x5F608315;
const NVAPI_GPU_CLIENT_POWER_TOPOLOGY_GET_STATUS: u32 = 0xEDCF624E;

// ─── NVAPI types ─────────────────────────────────────────────────────────────

type NvAPI_QueryInterface = unsafe extern "C" fn(id: u32) -> *const c_void;
type NvAPI_Initialize = unsafe extern "C" fn() -> i32;
type NvAPI_EnumPhysicalGPUs = unsafe extern "C" fn(handles: *mut [usize; 64], count: *mut u32) -> i32;
type NvAPI_GPU_GetUsages = unsafe extern "C" fn(handle: usize, usages: *mut GpuUsages) -> i32;
type NvAPI_GPU_GetThermalSettings = unsafe extern "C" fn(handle: usize, sensor: u32, settings: *mut ThermalSettings) -> i32;
type NvAPI_GPU_GetAllClockFrequencies = unsafe extern "C" fn(handle: usize, clocks: *mut ClockFrequencies) -> i32;
type NvAPI_GPU_GetMemoryInfoEx = unsafe extern "C" fn(handle: usize, info: *mut MemoryInfoEx) -> i32;
type NvAPI_GPU_GetTachReading = unsafe extern "C" fn(handle: usize, rpm: *mut u32) -> i32;
type NvAPI_GPU_ClientPowerTopologyGetStatus = unsafe extern "C" fn(handle: usize, status: *mut PowerTopologyStatus) -> i32;

const NVAPI_OK: i32 = 0;

// ─── NVAPI structs ───────────────────────────────────────────────────────────

#[repr(C)]
struct GpuUsages {
    version: u32,
    usages: [u32; 34], // [0] = version echoed, [3] = GPU usage %
}

impl GpuUsages {
    fn new() -> Self {
        let mut s = Self {
            version: 0,
            usages: [0; 34],
        };
        // Version: struct size | version 1
        s.version = (mem::size_of::<Self>() as u32) | (1 << 16);
        s
    }
}

#[repr(C)]
struct ThermalSettings {
    version: u32,
    count: u32,
    sensors: [ThermalSensor; 3],
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ThermalSensor {
    controller: u32,
    default_min_temp: i32,
    default_max_temp: i32,
    current_temp: i32,
    target: u32,
}

impl ThermalSettings {
    fn new() -> Self {
        Self {
            version: (mem::size_of::<Self>() as u32) | (2 << 16),
            count: 0,
            sensors: [ThermalSensor::default(); 3],
        }
    }
}

#[repr(C)]
struct ClockFrequencies {
    version: u32,
    clock_type: u32, // 0 = current
    entries: [ClockEntry; 32],
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ClockEntry {
    present: u32,
    frequency_khz: u32,
}

impl ClockFrequencies {
    fn new() -> Self {
        Self {
            version: (mem::size_of::<Self>() as u32) | (3 << 16),
            clock_type: 0, // current clocks
            entries: [ClockEntry::default(); 32],
        }
    }
}

#[repr(C)]
struct MemoryInfoEx {
    version: u32,
    dedicated_video_memory_kb: u32,
    available_dedicated_video_memory_kb: u32,
    system_video_memory_kb: u32,
    shared_system_memory_kb: u32,
    current_available_dedicated_video_memory_kb: u32,
    dedicated_video_memory_evictions_size_kb: u32,
    dedicated_video_memory_eviction_count: u32,
    dedicated_video_memory_promotions_size_kb: u32,
    dedicated_video_memory_promotion_count: u32,
}

impl MemoryInfoEx {
    fn new() -> Self {
        let mut s: Self = unsafe { mem::zeroed() };
        s.version = (mem::size_of::<Self>() as u32) | (1 << 16);
        s
    }
}

#[repr(C)]
struct PowerTopologyStatus {
    version: u32,
    count: u32,
    entries: [PowerTopologyEntry; 4],
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct PowerTopologyEntry {
    domain: u32,
    power_usage_mw: u32,
    power_budget_mw: u32,
}

impl PowerTopologyStatus {
    fn new() -> Self {
        Self {
            version: (mem::size_of::<Self>() as u32) | (1 << 16),
            count: 0,
            entries: [PowerTopologyEntry::default(); 4],
        }
    }
}

// ─── GpuPoller ───────────────────────────────────────────────────────────────

pub struct GpuPoller {
    gpu_handle: usize,
    fn_get_usages: NvAPI_GPU_GetUsages,
    fn_get_thermal: NvAPI_GPU_GetThermalSettings,
    fn_get_clocks: NvAPI_GPU_GetAllClockFrequencies,
    fn_get_memory: NvAPI_GPU_GetMemoryInfoEx,
    fn_get_tach: NvAPI_GPU_GetTachReading,
    fn_get_power: Option<NvAPI_GPU_ClientPowerTopologyGetStatus>,
}

impl GpuPoller {
    /// Attempt to initialize NVAPI. Returns None if nvapi64.dll is not found
    /// or initialization fails (e.g., AMD GPU system).
    pub fn new() -> Option<Self> {
        unsafe { Self::init_nvapi() }
    }

    unsafe fn init_nvapi() -> Option<Self> {
        // Load nvapi64.dll
        let module = LoadLibraryA(s!("nvapi64.dll")).ok()?;

        // Get the single exported function
        let query_interface: NvAPI_QueryInterface = mem::transmute(
            GetProcAddress(module.into(), s!("nvapi_QueryInterface"))?
        );

        // Resolve NvAPI_Initialize
        let initialize: NvAPI_Initialize = mem::transmute(
            query_interface(NVAPI_INITIALIZE)
        );

        if initialize() != NVAPI_OK {
            warn!("NVAPI: NvAPI_Initialize failed");
            return None;
        }

        // Resolve all needed functions
        let fn_enum_gpus: NvAPI_EnumPhysicalGPUs = mem::transmute(
            query_interface(NVAPI_ENUM_PHYSICAL_GPUS)
        );
        let fn_get_usages: NvAPI_GPU_GetUsages = mem::transmute(
            query_interface(NVAPI_GPU_GET_USAGES)
        );
        let fn_get_thermal: NvAPI_GPU_GetThermalSettings = mem::transmute(
            query_interface(NVAPI_GPU_GET_THERMAL_SETTINGS)
        );
        let fn_get_clocks: NvAPI_GPU_GetAllClockFrequencies = mem::transmute(
            query_interface(NVAPI_GPU_GET_ALL_CLOCK_FREQUENCIES)
        );
        let fn_get_memory: NvAPI_GPU_GetMemoryInfoEx = mem::transmute(
            query_interface(NVAPI_GPU_GET_MEMORY_INFO_EX)
        );
        let fn_get_tach: NvAPI_GPU_GetTachReading = mem::transmute(
            query_interface(NVAPI_GPU_GET_TACH_READING)
        );

        // Power topology is optional (GTX 900+ only)
        let fn_get_power_ptr = query_interface(NVAPI_GPU_CLIENT_POWER_TOPOLOGY_GET_STATUS);
        let fn_get_power: Option<NvAPI_GPU_ClientPowerTopologyGetStatus> = if fn_get_power_ptr.is_null() {
            info!("NVAPI: power topology not available (pre-GTX 900 GPU)");
            None
        } else {
            Some(mem::transmute(fn_get_power_ptr))
        };

        // Enumerate GPUs — use the first one
        let mut handles = [0usize; 64];
        let mut count = 0u32;
        if fn_enum_gpus(&mut handles, &mut count) != NVAPI_OK || count == 0 {
            warn!("NVAPI: no GPUs found");
            return None;
        }

        let gpu_handle = handles[0];
        info!(gpu_count = count, "NVAPI: initialized, using GPU 0");

        Some(Self {
            gpu_handle,
            fn_get_usages,
            fn_get_thermal,
            fn_get_clocks,
            fn_get_memory,
            fn_get_tach,
            fn_get_power,
        })
    }

    /// Poll all GPU sensors and return a populated GpuData struct.
    pub fn poll(&self) -> GpuData {
        let mut data = GpuData::default();

        unsafe {
            // Usage
            let mut usages = GpuUsages::new();
            if (self.fn_get_usages)(self.gpu_handle, &mut usages) == NVAPI_OK {
                data.usage_percent = usages.usages[3] as f32;
            }

            // Temperature
            let mut thermal = ThermalSettings::new();
            if (self.fn_get_thermal)(self.gpu_handle, 0, &mut thermal) == NVAPI_OK {
                if thermal.count > 0 {
                    data.temp_c = thermal.sensors[0].current_temp as f32;
                }
            }

            // Clock frequencies (index 0 = graphics, index 8 = memory)
            let mut clocks = ClockFrequencies::new();
            if (self.fn_get_clocks)(self.gpu_handle, &mut clocks) == NVAPI_OK {
                if clocks.entries[0].present != 0 {
                    data.core_clock_mhz = clocks.entries[0].frequency_khz / 1000;
                }
                if clocks.entries[8].present != 0 {
                    data.mem_clock_mhz = clocks.entries[8].frequency_khz / 1000;
                }
            }

            // Memory
            let mut mem_info = MemoryInfoEx::new();
            if (self.fn_get_memory)(self.gpu_handle, &mut mem_info) == NVAPI_OK {
                data.vram_total_mb = mem_info.dedicated_video_memory_kb / 1024;
                let available_kb = mem_info.current_available_dedicated_video_memory_kb;
                let total_kb = mem_info.dedicated_video_memory_kb;
                data.vram_used_mb = (total_kb.saturating_sub(available_kb)) / 1024;
            }

            // Fan speed
            let mut rpm = 0u32;
            if (self.fn_get_tach)(self.gpu_handle, &mut rpm) == NVAPI_OK {
                data.fan_speed_rpm = rpm;
            }

            // Power draw (GTX 900+ only)
            if let Some(fn_get_power) = self.fn_get_power {
                let mut power = PowerTopologyStatus::new();
                if fn_get_power(self.gpu_handle, &mut power) == NVAPI_OK && power.count > 0 {
                    data.power_draw_w = power.entries[0].power_usage_mw as f32 / 1000.0;
                }
            }
        }

        data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_poller_init_returns_some_on_nvidia() {
        // This test only passes on NVIDIA systems.
        // On AMD/Intel, it returns None — which is correct behavior.
        let poller = GpuPoller::new();
        if poller.is_some() {
            let data = poller.unwrap().poll();
            // Temperature should be a reasonable value if GPU is present
            assert!(data.temp_c > 0.0 && data.temp_c < 120.0,
                "GPU temp should be between 0-120°C, got {}", data.temp_c);
        }
        // If None, that's fine — no NVIDIA GPU
    }

    #[test]
    fn gpu_usages_struct_version() {
        let usages = GpuUsages::new();
        // Version encodes struct size in low 16 bits
        let size = (usages.version & 0xFFFF) as usize;
        assert_eq!(size, mem::size_of::<GpuUsages>());
    }

    #[test]
    fn thermal_settings_struct_version() {
        let thermal = ThermalSettings::new();
        let size = (thermal.version & 0xFFFF) as usize;
        assert_eq!(size, mem::size_of::<ThermalSettings>());
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Compiles (gpu.rs isn't wired into mod.rs yet, but should compile as a standalone module once added).

Note: The module needs to be added to `sensors/mod.rs` — that happens in Task 5.

- [ ] **Step 3: Commit**

```bash
git add host/src/sensors/gpu.rs
git commit -m "feat(host): add NVAPI FFI bindings for GPU sensor polling"
```

---

### Task 3: WMI CPU Temperature Sensor

**Files:**
- Create: `host/src/sensors/cpu_temp.rs`

- [ ] **Step 1: Create host/src/sensors/cpu_temp.rs**

```rust
//! CPU temperature via WMI MSAcpi_ThermalZoneTemperature.
//!
//! This is a built-in Windows WMI class that some systems expose without
//! third-party software. Returns temperature in tenths of Kelvin.
//! If the query fails (common on many systems), returns f32::NAN.

use serde::Deserialize;
use tracing::{info, warn};
use wmi::{COMLibrary, WMIConnection};

#[derive(Deserialize)]
#[serde(rename = "MSAcpi_ThermalZoneTemperature")]
#[serde(rename_all = "PascalCase")]
struct ThermalZone {
    current_temperature: u32, // tenths of Kelvin
}

/// Converts WMI thermal zone value (tenths of Kelvin) to Celsius.
pub fn tenths_kelvin_to_celsius(tenths_k: u32) -> f32 {
    (tenths_k as f32 / 10.0) - 273.15
}

pub struct CpuTempPoller {
    connection: Option<WMIConnection>,
}

impl CpuTempPoller {
    /// Initialize WMI connection to root\WMI namespace.
    /// Returns a poller that always works — if WMI connection fails,
    /// poll() will return NaN.
    pub fn new() -> Self {
        let connection = Self::connect();
        Self { connection }
    }

    fn connect() -> Option<WMIConnection> {
        let com = COMLibrary::without_security().ok()?;
        match WMIConnection::with_namespace_path("root\\WMI", com) {
            Ok(conn) => {
                info!("WMI: connected to root\\WMI for CPU temperature");
                Some(conn)
            }
            Err(e) => {
                warn!(error = %e, "WMI: failed to connect to root\\WMI — CPU temperature unavailable");
                None
            }
        }
    }

    /// Query CPU temperature. Returns Celsius or NaN if unavailable.
    pub fn poll(&self) -> f32 {
        let conn = match &self.connection {
            Some(c) => c,
            None => return f32::NAN,
        };

        match conn.raw_query::<ThermalZone>("SELECT CurrentTemperature FROM MSAcpi_ThermalZoneTemperature") {
            Ok(results) if !results.is_empty() => {
                tenths_kelvin_to_celsius(results[0].current_temperature)
            }
            Ok(_) => f32::NAN, // Query succeeded but no results
            Err(_) => f32::NAN, // Query failed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenths_kelvin_conversion() {
        // 3000 tenths of Kelvin = 300K = 26.85°C
        let celsius = tenths_kelvin_to_celsius(3000);
        assert!((celsius - 26.85).abs() < 0.01, "Expected ~26.85°C, got {celsius}");
    }

    #[test]
    fn boiling_point_conversion() {
        // 3731 tenths of Kelvin = 373.1K = 99.95°C
        let celsius = tenths_kelvin_to_celsius(3731);
        assert!((celsius - 99.95).abs() < 0.01, "Expected ~99.95°C, got {celsius}");
    }

    #[test]
    fn cpu_temp_poller_does_not_panic() {
        // Just verify it doesn't crash — result may be NaN on systems without WMI thermal
        let poller = CpuTempPoller::new();
        let temp = poller.poll();
        // temp is either a valid number or NaN — both are fine
        assert!(temp.is_nan() || (temp > -50.0 && temp < 150.0),
            "Temperature should be NaN or reasonable, got {temp}");
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Compiles. Note: `wmi` requires `serde::Deserialize` on the struct.

- [ ] **Step 3: Commit**

```bash
git add host/src/sensors/cpu_temp.rs
git commit -m "feat(host): add WMI CPU temperature sensor"
```

---

### Task 4: RAM Sensor

**Files:**
- Create: `host/src/sensors/ram.rs`

- [ ] **Step 1: Create host/src/sensors/ram.rs**

```rust
//! RAM sensor via sysinfo.

use omni_shared::RamData;
use sysinfo::System;

pub struct RamPoller;

impl RamPoller {
    pub fn new() -> Self {
        Self
    }

    /// Poll RAM data from a shared sysinfo::System instance.
    /// The System must have been refreshed with refresh_memory() before calling.
    pub fn poll(&self, system: &System) -> RamData {
        let total_bytes = system.total_memory();
        let used_bytes = system.used_memory();

        let total_mb = total_bytes / (1024 * 1024);
        let used_mb = used_bytes / (1024 * 1024);

        let usage_percent = if total_mb > 0 {
            (used_mb as f32 / total_mb as f32) * 100.0
        } else {
            0.0
        };

        RamData {
            usage_percent,
            used_mb: used_mb,
            total_mb: total_mb,
            frequency_mhz: 0,   // requires LHM/WMI — deferred
            timing_cl: 0,        // requires LHM/WMI — deferred
            temp_c: f32::NAN,    // requires LHM/WMI — deferred
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_poller_returns_nonzero() {
        let mut system = System::new();
        system.refresh_memory();
        let poller = RamPoller::new();
        let data = poller.poll(&system);

        assert!(data.total_mb > 0, "System should have some RAM");
        assert!(data.used_mb > 0, "Some RAM should be in use");
        assert!(data.usage_percent > 0.0 && data.usage_percent <= 100.0,
            "Usage should be 0-100%, got {}", data.usage_percent);
    }

    #[test]
    fn ram_deferred_fields_are_unavailable() {
        let mut system = System::new();
        system.refresh_memory();
        let poller = RamPoller::new();
        let data = poller.poll(&system);

        assert_eq!(data.frequency_mhz, 0);
        assert_eq!(data.timing_cl, 0);
        assert!(data.temp_c.is_nan());
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add host/src/sensors/ram.rs
git commit -m "feat(host): add RAM sensor via sysinfo"
```

---

### Task 5: Integrate All Sensors into SensorPoller

**Files:**
- Modify: `host/src/sensors/mod.rs`
- Modify: `host/src/sensors/cpu.rs`

The `SensorPoller` thread now creates all sensor modules and polls them each cycle. The `sysinfo::System` instance is shared between CPU and RAM polling.

- [ ] **Step 1: Update cpu.rs to accept a shared System reference**

Replace the `CpuPoller` to accept a `&System` in `poll()` instead of owning its own:

```rust
use omni_shared::CpuData;
use sysinfo::System;
use tracing::info;

pub struct CpuPoller;

impl CpuPoller {
    pub fn new(system: &System) -> Self {
        let core_count = system.cpus().len();
        info!(core_count, "sysinfo: CPU sensor initialized");
        Self
    }

    /// Read CPU data from a shared sysinfo::System instance.
    /// The System must have been refreshed with refresh_cpu_all() before calling.
    pub fn poll(&self, system: &System) -> CpuData {
        let cpus = system.cpus();
        let core_count = cpus.len().min(32) as u32;

        let mut per_core_usage = [-1.0f32; 32];
        let mut per_core_freq_mhz = [0u32; 32];

        for (i, cpu) in cpus.iter().enumerate().take(32) {
            per_core_usage[i] = cpu.cpu_usage();
            per_core_freq_mhz[i] = cpu.frequency() as u32;
        }

        let total_usage = system.global_cpu_usage();

        CpuData {
            total_usage_percent: total_usage,
            per_core_usage,
            core_count,
            per_core_freq_mhz,
            package_temp_c: f32::NAN, // set by CpuTempPoller separately
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_poller_returns_valid_core_count() {
        let mut system = System::new();
        system.refresh_cpu_all();
        std::thread::sleep(std::time::Duration::from_millis(200));
        system.refresh_cpu_all();
        let poller = CpuPoller::new(&system);
        let data = poller.poll(&system);
        assert!(data.core_count > 0, "Should detect at least one CPU core");
        assert!(data.core_count <= 32, "Core count should be capped at 32");
    }

    #[test]
    fn cpu_poller_unused_cores_are_negative() {
        let mut system = System::new();
        system.refresh_cpu_all();
        std::thread::sleep(std::time::Duration::from_millis(200));
        system.refresh_cpu_all();
        let poller = CpuPoller::new(&system);
        let data = poller.poll(&system);
        for i in data.core_count as usize..32 {
            assert_eq!(data.per_core_usage[i], -1.0);
        }
    }

    #[test]
    fn cpu_poller_temp_is_nan() {
        let mut system = System::new();
        system.refresh_cpu_all();
        let poller = CpuPoller::new(&system);
        let data = poller.poll(&system);
        assert!(data.package_temp_c.is_nan(), "Temp should be NaN (set by CpuTempPoller)");
    }
}
```

- [ ] **Step 2: Update sensors/mod.rs to integrate all sensors**

Replace the entire file:

```rust
pub mod cpu;
pub mod cpu_temp;
pub mod gpu;
pub mod ram;

use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use omni_shared::SensorSnapshot;
use sysinfo::System;
use tracing::info;

use cpu::CpuPoller;
use cpu_temp::CpuTempPoller;
use gpu::GpuPoller;
use ram::RamPoller;

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
            // Shared sysinfo instance for CPU and RAM
            let mut system = System::new();
            system.refresh_cpu_all();
            system.refresh_memory();

            let cpu = CpuPoller::new(&system);
            let cpu_temp = CpuTempPoller::new();
            let gpu = GpuPoller::new();
            let ram = RamPoller::new();

            if gpu.is_some() {
                info!("sensor suite: CPU + GPU (NVAPI) + RAM + CPU temp (WMI)");
            } else {
                info!("sensor suite: CPU + RAM + CPU temp (WMI) — no NVIDIA GPU detected");
            }

            // sysinfo needs two samples to compute CPU usage
            thread::sleep(Duration::from_millis(500));

            info!("Sensor polling started");

            while running_clone.load(Ordering::Relaxed) {
                system.refresh_cpu_all();
                system.refresh_memory();

                let mut cpu_data = cpu.poll(&system);
                cpu_data.package_temp_c = cpu_temp.poll();

                let gpu_data = match &gpu {
                    Some(g) => g.poll(),
                    None => omni_shared::GpuData::default(),
                };

                let ram_data = ram.poll(&system);

                let snapshot = SensorSnapshot {
                    timestamp_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64,
                    cpu: cpu_data,
                    gpu: gpu_data,
                    ram: ram_data,
                    ..Default::default() // frame data — Phase 8
                };

                if tx.send(snapshot).is_err() {
                    break;
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

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p omni-host`
Expected: All tests pass (cpu, cpu_temp, gpu, ram, config, scanner, ipc).

- [ ] **Step 4: Commit**

```bash
git add host/src/sensors/
git commit -m "feat(host): integrate all sensors (CPU, GPU, RAM, temp) into poller"
```

---

### Task 6: Widget Builder

**Files:**
- Create: `host/src/widget_builder.rs`
- Modify: `host/src/main.rs`

- [ ] **Step 1: Create host/src/widget_builder.rs**

```rust
//! Builds ComputedWidget arrays from sensor snapshots.
//!
//! Phase 7: hardcoded sensor widget list.
//! Phase 9a: replaced by .widget file parsing + taffy layout engine.

use omni_shared::{ComputedWidget, SensorSnapshot, WidgetType, SensorSource, write_fixed_str};

/// Builds widgets from sensor data.
/// In Phase 9a, this gains a constructor that takes a parsed widget tree + theme.
pub struct WidgetBuilder;

impl WidgetBuilder {
    pub fn new() -> Self {
        Self
    }

    /// Build the widget array for one frame.
    /// In Phase 9a, this resolves styles, runs taffy layout, evaluates animations.
    pub fn build(&self, snapshot: &SensorSnapshot) -> Vec<ComputedWidget> {
        let mut widgets = Vec::new();
        let x = 20.0;
        let mut y = 20.0;
        let row_height = 28.0;
        let width = 260.0;

        // CPU Usage
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::CpuUsage,
            &format!("CPU: {:.0}%", snapshot.cpu.total_usage_percent),
        ));
        y += row_height;

        // CPU Temp
        let temp_text = if snapshot.cpu.package_temp_c.is_nan() {
            "CPU Temp: N/A".to_string()
        } else {
            format!("CPU Temp: {:.0}°C", snapshot.cpu.package_temp_c)
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::CpuTemp,
            &temp_text,
        ));
        y += row_height;

        // GPU Usage
        let gpu_text = if snapshot.gpu.usage_percent > 0.0 || snapshot.gpu.temp_c > 0.0 {
            format!("GPU: {:.0}%", snapshot.gpu.usage_percent)
        } else {
            "GPU: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuUsage,
            &gpu_text,
        ));
        y += row_height;

        // GPU Temp
        let gpu_temp_text = if snapshot.gpu.temp_c > 0.0 {
            format!("GPU Temp: {:.0}°C", snapshot.gpu.temp_c)
        } else {
            "GPU Temp: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuTemp,
            &gpu_temp_text,
        ));
        y += row_height;

        // GPU Clock
        let gpu_clock_text = if snapshot.gpu.core_clock_mhz > 0 {
            format!("GPU Clock: {} MHz", snapshot.gpu.core_clock_mhz)
        } else {
            "GPU Clock: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuClock,
            &gpu_clock_text,
        ));
        y += row_height;

        // VRAM
        let vram_text = if snapshot.gpu.vram_total_mb > 0 {
            format!("VRAM: {}/{} MB", snapshot.gpu.vram_used_mb, snapshot.gpu.vram_total_mb)
        } else {
            "VRAM: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuVram,
            &vram_text,
        ));
        y += row_height;

        // GPU Power
        let power_text = if snapshot.gpu.power_draw_w > 0.0 {
            format!("GPU Power: {:.0}W", snapshot.gpu.power_draw_w)
        } else {
            "GPU Power: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuPower,
            &power_text,
        ));
        y += row_height;

        // RAM
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::RamUsage,
            &format!("RAM: {:.0}% ({}/{} MB)", snapshot.ram.usage_percent, snapshot.ram.used_mb, snapshot.ram.total_mb),
        ));
        y += row_height;

        // FPS (Phase 8 — always N/A for now)
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::Fps,
            "FPS: N/A",
        ));

        widgets
    }

    fn make_sensor_widget(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        source: SensorSource,
        text: &str,
    ) -> ComputedWidget {
        let mut widget = ComputedWidget::default();
        widget.widget_type = WidgetType::SensorValue;
        widget.source = source;
        widget.x = x;
        widget.y = y;
        widget.width = width;
        widget.height = height;
        widget.font_size = 16.0;
        widget.font_weight = 400;
        widget.color_rgba = [255, 255, 255, 255];
        widget.bg_color_rgba = [20, 20, 20, 180];
        widget.border_radius = [4.0; 4];
        widget.opacity = 1.0;
        write_fixed_str(&mut widget.format_pattern, text);
        widget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_correct_number_of_widgets() {
        let builder = WidgetBuilder::new();
        let snapshot = SensorSnapshot::default();
        let widgets = builder.build(&snapshot);
        assert_eq!(widgets.len(), 9, "Should produce 9 sensor widgets");
    }

    #[test]
    fn widgets_are_vertically_stacked() {
        let builder = WidgetBuilder::new();
        let snapshot = SensorSnapshot::default();
        let widgets = builder.build(&snapshot);
        for i in 1..widgets.len() {
            assert!(widgets[i].y > widgets[i - 1].y,
                "Widget {} should be below widget {}", i, i - 1);
        }
    }

    #[test]
    fn unavailable_sensors_show_na() {
        let builder = WidgetBuilder::new();
        let snapshot = SensorSnapshot::default(); // all defaults — GPU zeroed, temp NaN
        let widgets = builder.build(&snapshot);

        // CPU temp (NaN) should show N/A
        let cpu_temp_text = omni_shared::read_fixed_str(&widgets[1].format_pattern);
        assert!(cpu_temp_text.contains("N/A"), "CPU temp should be N/A, got: {cpu_temp_text}");

        // GPU usage (0.0) should show N/A
        let gpu_text = omni_shared::read_fixed_str(&widgets[2].format_pattern);
        assert!(gpu_text.contains("N/A"), "GPU should be N/A, got: {gpu_text}");

        // FPS should always be N/A in this phase
        let fps_text = omni_shared::read_fixed_str(&widgets[8].format_pattern);
        assert!(fps_text.contains("N/A"), "FPS should be N/A, got: {fps_text}");
    }

    #[test]
    fn available_sensors_show_values() {
        let builder = WidgetBuilder::new();
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 42.0;
        snapshot.gpu.usage_percent = 83.0;
        snapshot.gpu.temp_c = 71.0;
        snapshot.ram.usage_percent = 62.0;
        snapshot.ram.used_mb = 16000;
        snapshot.ram.total_mb = 32000;

        let widgets = builder.build(&snapshot);

        let cpu_text = omni_shared::read_fixed_str(&widgets[0].format_pattern);
        assert!(cpu_text.contains("42"), "CPU should show 42%, got: {cpu_text}");

        let gpu_text = omni_shared::read_fixed_str(&widgets[2].format_pattern);
        assert!(gpu_text.contains("83"), "GPU should show 83%, got: {gpu_text}");

        let ram_text = omni_shared::read_fixed_str(&widgets[7].format_pattern);
        assert!(ram_text.contains("62"), "RAM should show 62%, got: {ram_text}");
    }
}
```

- [ ] **Step 2: Update main.rs to use WidgetBuilder**

In `main.rs`, add `mod widget_builder;` after the other module declarations.

Replace the watch loop's widget building and the `build_cpu_widget` function:

Replace:
```rust
        // Build hardcoded CPU usage widget
        let widget = build_cpu_widget(&latest_snapshot);

        // Write to shared memory
        shm_writer.write(&latest_snapshot, &[widget], 1);
```

With:
```rust
        // Build sensor widgets
        let widgets = widget_builder.build(&latest_snapshot);

        // Write to shared memory
        shm_writer.write(&latest_snapshot, &widgets, 1);
```

And add `let widget_builder = widget_builder::WidgetBuilder::new();` before the loop.

Delete the `build_cpu_widget` function entirely.

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/widget_builder.rs host/src/main.rs
git commit -m "feat(host): add WidgetBuilder abstraction, display all sensor widgets"
```

---

### Task 7: Integration Test — Full Sensor Dashboard

This is a manual integration test.

- [ ] **Step 1: Build everything**

```bash
cargo build -p omni-host && cargo build -p omni-overlay-dll
```

- [ ] **Step 2: Start host and launch a game**

```bash
cargo run -p omni-host -- --watch target/debug/omni_overlay_dll.dll
```

Check host logs for sensor initialization:
```
INFO sysinfo: CPU sensor initialized core_count=...
INFO NVAPI: initialized, using GPU 0
INFO WMI: connected to root\WMI for CPU temperature
INFO sensor suite: CPU + GPU (NVAPI) + RAM + CPU temp (WMI)
INFO Sensor polling started
```

- [ ] **Step 3: Verify overlay shows all sensors**

Launch a DX11 or DX12 game. Verify the overlay shows:
- CPU usage (updating)
- CPU temp (value or N/A)
- GPU usage, temp, clock, VRAM, power (values on NVIDIA, N/A otherwise)
- RAM usage with MB values
- FPS: N/A

- [ ] **Step 4: Verify resilience**

- Ctrl+C → restart host → overlay reappears with all sensors
- Kill via Task Manager → restart → reconnects
- Close game → reopen → overlay binds correctly

- [ ] **Step 5: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 7 integration test"
```

---

## Phase 7 Complete — Summary

At this point you have:

1. **CPU sensor** — usage, per-core, frequencies via sysinfo
2. **CPU temperature** — WMI MSAcpi_ThermalZoneTemperature (N/A if unavailable)
3. **GPU sensor** — usage, temp, clocks, VRAM, fan, power draw via NVAPI FFI (N/A if no NVIDIA GPU)
4. **RAM sensor** — usage, used/total via sysinfo
5. **WidgetBuilder abstraction** — ready for Phase 9a drop-in replacement
6. **N/A handling** — unavailable sensors display "N/A" gracefully
7. **Sensor binding ledger** — logs which sensors initialized successfully

**Next:** Phase 8 adds ETW frame timing (FPS, frame time, percentiles).
