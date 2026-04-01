//! Sensor data types shared between host and overlay DLL.
//! All structs are #[repr(C)] because they cross process boundaries via shared memory.

use ts_rs::TS;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
pub struct SensorSnapshot {
    pub timestamp_ms: u64,
    pub cpu: CpuData,
    pub gpu: GpuData,
    pub ram: RamData,
    pub frame: FrameData,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
pub struct CpuData {
    /// Overall CPU usage as a percentage (0.0–100.0).
    pub total_usage_percent: f32,
    /// Per-core usage percentages. Unused cores are set to -1.0.
    #[ts(type = "number[]")]
    pub per_core_usage: [f32; 32],
    pub core_count: u32,
    /// Per-core frequency in MHz. Unused cores are 0.
    #[ts(type = "number[]")]
    pub per_core_freq_mhz: [u32; 32],
    /// CPU package temperature in Celsius. f32::NAN if unavailable.
    pub package_temp_c: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
pub struct GpuData {
    pub usage_percent: f32,
    pub temp_c: f32,
    pub core_clock_mhz: u32,
    pub mem_clock_mhz: u32,
    pub vram_used_mb: u32,
    pub vram_total_mb: u32,
    pub fan_speed_percent: u32,
    pub power_draw_w: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
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
#[derive(Clone, Copy, Debug, TS)]
#[ts(export, export_to = "../../desktop/src/generated/")]
pub struct FrameData {
    pub fps: f32,
    pub frame_time_ms: f32,
    pub frame_time_avg_ms: f32,
    pub frame_time_1percent_ms: f32,
    pub frame_time_01percent_ms: f32,
    /// false if no frame data source is active.
    pub available: bool,
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
            fan_speed_percent: 0,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn sensor_snapshot_is_repr_c_sized() {
        let size = mem::size_of::<SensorSnapshot>();
        assert!(size > 0);
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
