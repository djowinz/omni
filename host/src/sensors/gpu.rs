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
            GetProcAddress(module, s!("nvapi_QueryInterface"))?
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
