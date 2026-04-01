//! NVIDIA GPU sensor via NVML (NVIDIA Management Library).
//!
//! NVML is a documented, stable API that ships with every NVIDIA driver.
//! Unlike NVAPI's undocumented QueryInterface, NVML has proper exported
//! functions that work reliably across all GPU generations.
//!
//! nvml.dll is loaded at runtime via LoadLibrary + GetProcAddress.

use std::ffi::c_void;
use std::mem;

use omni_shared::GpuData;
use tracing::{info, warn};
use windows::core::s;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

// ─── NVML constants ──────────────────────────────────────────────────────────

const NVML_SUCCESS: u32 = 0;
const NVML_TEMPERATURE_GPU: u32 = 0;
const NVML_CLOCK_GRAPHICS: u32 = 0;
const NVML_CLOCK_MEM: u32 = 2;

// ─── NVML types ──────────────────────────────────────────────────────────────

/// Opaque device handle (pointer-sized).
type NvmlDevice = *mut c_void;

type NvmlInit = unsafe extern "C" fn() -> u32;
type NvmlDeviceGetCount = unsafe extern "C" fn(count: *mut u32) -> u32;
type NvmlDeviceGetHandleByIndex = unsafe extern "C" fn(index: u32, device: *mut NvmlDevice) -> u32;
type NvmlDeviceGetUtilizationRates =
    unsafe extern "C" fn(device: NvmlDevice, utilization: *mut NvmlUtilization) -> u32;
type NvmlDeviceGetTemperature =
    unsafe extern "C" fn(device: NvmlDevice, sensor_type: u32, temp: *mut u32) -> u32;
type NvmlDeviceGetClockInfo =
    unsafe extern "C" fn(device: NvmlDevice, clock_type: u32, clock_mhz: *mut u32) -> u32;
type NvmlDeviceGetMemoryInfo =
    unsafe extern "C" fn(device: NvmlDevice, memory: *mut NvmlMemory) -> u32;
type NvmlDeviceGetFanSpeed = unsafe extern "C" fn(device: NvmlDevice, speed: *mut u32) -> u32;
type NvmlDeviceGetPowerUsage = unsafe extern "C" fn(device: NvmlDevice, power_mw: *mut u32) -> u32;
type NvmlDeviceGetName =
    unsafe extern "C" fn(device: NvmlDevice, name: *mut u8, length: u32) -> u32;

// ─── NVML structs ────────────────────────────────────────────────────────────

#[repr(C)]
struct NvmlUtilization {
    gpu: u32,    // GPU utilization percentage
    memory: u32, // Memory controller utilization percentage
}

#[repr(C)]
struct NvmlMemory {
    total: u64, // Total VRAM in bytes
    free: u64,  // Free VRAM in bytes
    used: u64,  // Used VRAM in bytes
}

// ─── GpuPoller ───────────────────────────────────────────────────────────────

pub struct GpuPoller {
    device: NvmlDevice,
    fn_get_utilization: NvmlDeviceGetUtilizationRates,
    fn_get_temperature: NvmlDeviceGetTemperature,
    fn_get_clock: NvmlDeviceGetClockInfo,
    fn_get_memory: NvmlDeviceGetMemoryInfo,
    fn_get_fan_speed: NvmlDeviceGetFanSpeed,
    fn_get_power: NvmlDeviceGetPowerUsage,
}

// SAFETY: NvmlDevice is a pointer used only by the polling thread.
unsafe impl Send for GpuPoller {}

impl GpuPoller {
    /// Attempt to initialize NVML. Returns None if nvml.dll is not found
    /// or initialization fails (e.g., AMD GPU system).
    pub fn new() -> Option<Self> {
        unsafe { Self::init_nvml() }
    }

    /// # Safety
    ///
    /// All function pointers are resolved via `GetProcAddress`; a null return
    /// becomes `None` and causes an early exit via `?`, so no dangling pointer
    /// is ever stored. The NVML API is documented and stable across NVIDIA
    /// driver versions.
    unsafe fn init_nvml() -> Option<Self> {
        // Try loading nvml.dll — it's in System32 on modern NVIDIA drivers
        let module = LoadLibraryA(s!("nvml.dll"))
            .or_else(|_| LoadLibraryA(s!("C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvml.dll")))
            .ok()?;

        // Resolve all functions
        let fn_init: NvmlInit = mem::transmute(GetProcAddress(module, s!("nvmlInit_v2"))?);
        let fn_get_count: NvmlDeviceGetCount =
            mem::transmute(GetProcAddress(module, s!("nvmlDeviceGetCount_v2"))?);
        let fn_get_handle: NvmlDeviceGetHandleByIndex =
            mem::transmute(GetProcAddress(module, s!("nvmlDeviceGetHandleByIndex_v2"))?);
        let fn_get_utilization: NvmlDeviceGetUtilizationRates =
            mem::transmute(GetProcAddress(module, s!("nvmlDeviceGetUtilizationRates"))?);
        let fn_get_temperature: NvmlDeviceGetTemperature =
            mem::transmute(GetProcAddress(module, s!("nvmlDeviceGetTemperature"))?);
        let fn_get_clock: NvmlDeviceGetClockInfo =
            mem::transmute(GetProcAddress(module, s!("nvmlDeviceGetClockInfo"))?);
        let fn_get_memory: NvmlDeviceGetMemoryInfo =
            mem::transmute(GetProcAddress(module, s!("nvmlDeviceGetMemoryInfo"))?);
        let fn_get_fan_speed: NvmlDeviceGetFanSpeed =
            mem::transmute(GetProcAddress(module, s!("nvmlDeviceGetFanSpeed"))?);
        let fn_get_power: NvmlDeviceGetPowerUsage =
            mem::transmute(GetProcAddress(module, s!("nvmlDeviceGetPowerUsage"))?);
        let fn_get_name: NvmlDeviceGetName =
            mem::transmute(GetProcAddress(module, s!("nvmlDeviceGetName"))?);

        // Initialize NVML
        let result = fn_init();
        if result != NVML_SUCCESS {
            warn!(error_code = result, "NVML: nvmlInit_v2 failed");
            return None;
        }

        // Get device count
        let mut count = 0u32;
        let result = fn_get_count(&mut count);
        if result != NVML_SUCCESS || count == 0 {
            warn!(error_code = result, "NVML: no GPUs found");
            return None;
        }

        // Get handle for GPU 0
        let mut device: NvmlDevice = std::ptr::null_mut();
        let result = fn_get_handle(0, &mut device);
        if result != NVML_SUCCESS {
            warn!(error_code = result, "NVML: failed to get GPU 0 handle");
            return None;
        }

        // Log GPU name
        let mut name_buf = [0u8; 256];
        if fn_get_name(device, name_buf.as_mut_ptr(), 256) == NVML_SUCCESS {
            let name_end = name_buf
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(name_buf.len());
            let name = String::from_utf8_lossy(&name_buf[..name_end]);
            info!(gpu_name = %name, gpu_count = count, "NVML: initialized");
        } else {
            info!(gpu_count = count, "NVML: initialized (GPU 0)");
        }

        Some(Self {
            device,
            fn_get_utilization,
            fn_get_temperature,
            fn_get_clock,
            fn_get_memory,
            fn_get_fan_speed,
            fn_get_power,
        })
    }

    /// Poll all GPU sensors and return a populated GpuData struct.
    pub fn poll(&self) -> GpuData {
        let mut data = GpuData::default();

        // SAFETY: All function pointers were validated during init_nvml (non-null,
        // correct signatures). self.device is a valid NVML handle obtained from
        // nvmlDeviceGetHandleByIndex. Each call writes to a stack-local out-parameter
        // of the correct type as specified by the NVML documentation.
        unsafe {
            // Utilization (GPU + memory controller)
            let mut util = NvmlUtilization { gpu: 0, memory: 0 };
            if (self.fn_get_utilization)(self.device, &mut util) == NVML_SUCCESS {
                data.usage_percent = util.gpu as f32;
            }

            // Temperature
            let mut temp = 0u32;
            if (self.fn_get_temperature)(self.device, NVML_TEMPERATURE_GPU, &mut temp)
                == NVML_SUCCESS
            {
                data.temp_c = temp as f32;
            }

            // Core clock
            let mut clock = 0u32;
            if (self.fn_get_clock)(self.device, NVML_CLOCK_GRAPHICS, &mut clock) == NVML_SUCCESS {
                data.core_clock_mhz = clock;
            }

            // Memory clock
            let mut mem_clock = 0u32;
            if (self.fn_get_clock)(self.device, NVML_CLOCK_MEM, &mut mem_clock) == NVML_SUCCESS {
                data.mem_clock_mhz = mem_clock;
            }

            // VRAM
            let mut memory = NvmlMemory {
                total: 0,
                free: 0,
                used: 0,
            };
            if (self.fn_get_memory)(self.device, &mut memory) == NVML_SUCCESS {
                data.vram_total_mb = (memory.total / (1024 * 1024)) as u32;
                data.vram_used_mb = (memory.used / (1024 * 1024)) as u32;
            }

            // Fan speed (percentage)
            let mut fan_speed = 0u32;
            if (self.fn_get_fan_speed)(self.device, &mut fan_speed) == NVML_SUCCESS {
                data.fan_speed_percent = fan_speed;
            }

            // Power draw (milliwatts → watts)
            let mut power_mw = 0u32;
            if (self.fn_get_power)(self.device, &mut power_mw) == NVML_SUCCESS {
                data.power_draw_w = power_mw as f32 / 1000.0;
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
        let poller = GpuPoller::new();
        if let Some(poller) = poller {
            let data = poller.poll();
            // Temperature should be reasonable if GPU is present
            assert!(
                data.temp_c > 0.0 && data.temp_c < 120.0,
                "GPU temp should be 0-120°C, got {}",
                data.temp_c
            );
            // Should have some VRAM
            assert!(data.vram_total_mb > 0, "GPU should report VRAM total");
        }
        // If None, that's fine — no NVIDIA GPU or nvml.dll not found
    }

    #[test]
    fn nvml_structs_are_correct_size() {
        assert_eq!(mem::size_of::<NvmlUtilization>(), 8); // two u32
        assert_eq!(mem::size_of::<NvmlMemory>(), 24); // three u64
    }
}
