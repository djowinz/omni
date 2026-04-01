//! Maps sensor path strings to SensorSource variants and extracts values.

use omni_shared::{SensorSnapshot, SensorSource};

/// Look up a sensor path string and return the matching SensorSource.
pub fn parse_sensor_path(path: &str) -> Option<SensorSource> {
    match path {
        "cpu.usage" => Some(SensorSource::CpuUsage),
        "cpu.temp" => Some(SensorSource::CpuTemp),
        "gpu.usage" => Some(SensorSource::GpuUsage),
        "gpu.temp" => Some(SensorSource::GpuTemp),
        "gpu.clock" => Some(SensorSource::GpuClock),
        "gpu.mem-clock" => Some(SensorSource::GpuMemClock),
        "gpu.vram" => Some(SensorSource::GpuVram),
        "gpu.vram.used" => Some(SensorSource::GpuVram),
        "gpu.vram.total" => Some(SensorSource::GpuVram),
        "gpu.power" => Some(SensorSource::GpuPower),
        "gpu.fan" => Some(SensorSource::GpuFan),
        "ram.usage" => Some(SensorSource::RamUsage),
        "fps" => Some(SensorSource::Fps),
        "frame-time" => Some(SensorSource::FrameTime),
        "frame-time.avg" => Some(SensorSource::FrameTimeAvg),
        "frame-time.1pct" => Some(SensorSource::FrameTime1Pct),
        "frame-time.01pct" => Some(SensorSource::FrameTime01Pct),
        _ => None,
    }
}

/// Get the formatted string value for a sensor path from a snapshot.
pub fn get_sensor_value(path: &str, snapshot: &SensorSnapshot) -> String {
    match path {
        "cpu.usage" => format!("{:.0}", snapshot.cpu.total_usage_percent),
        "cpu.temp" => format_temp(snapshot.cpu.package_temp_c),
        "gpu.usage" => format!("{:.0}", snapshot.gpu.usage_percent),
        "gpu.temp" => format_temp(snapshot.gpu.temp_c),
        "gpu.clock" => format!("{}", snapshot.gpu.core_clock_mhz),
        "gpu.mem-clock" => format!("{}", snapshot.gpu.mem_clock_mhz),
        "gpu.vram" => format!(
            "{}/{}",
            snapshot.gpu.vram_used_mb, snapshot.gpu.vram_total_mb
        ),
        "gpu.vram.used" => format!("{}", snapshot.gpu.vram_used_mb),
        "gpu.vram.total" => format!("{}", snapshot.gpu.vram_total_mb),
        "gpu.power" => format!("{:.0}", snapshot.gpu.power_draw_w),
        "gpu.fan" => format!("{}", snapshot.gpu.fan_speed_percent),
        "ram.usage" => format!("{:.0}", snapshot.ram.usage_percent),
        "ram.used" => format!("{}", snapshot.ram.used_mb),
        "ram.total" => format!("{}", snapshot.ram.total_mb),
        "fps" => {
            if snapshot.frame.available {
                format!("{:.0}", snapshot.frame.fps)
            } else {
                "N/A".to_string()
            }
        }
        "frame-time" => {
            if snapshot.frame.available {
                format!("{:.1}", snapshot.frame.frame_time_ms)
            } else {
                "N/A".to_string()
            }
        }
        "frame-time.avg" => {
            if snapshot.frame.available {
                format!("{:.1}", snapshot.frame.frame_time_avg_ms)
            } else {
                "N/A".to_string()
            }
        }
        "frame-time.1pct" => {
            if snapshot.frame.available {
                format!("{:.1}", snapshot.frame.frame_time_1percent_ms)
            } else {
                "N/A".to_string()
            }
        }
        "frame-time.01pct" => {
            if snapshot.frame.available {
                format!("{:.1}", snapshot.frame.frame_time_01percent_ms)
            } else {
                "N/A".to_string()
            }
        }
        _ => "N/A".to_string(),
    }
}

fn format_temp(temp_c: f32) -> String {
    if temp_c.is_nan() {
        "N/A".to_string()
    } else {
        format!("{:.0}", temp_c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_paths() {
        assert_eq!(parse_sensor_path("cpu.usage"), Some(SensorSource::CpuUsage));
        assert_eq!(parse_sensor_path("gpu.temp"), Some(SensorSource::GpuTemp));
        assert_eq!(parse_sensor_path("fps"), Some(SensorSource::Fps));
        assert_eq!(parse_sensor_path("nonexistent"), None);
    }

    #[test]
    fn get_value_formats_correctly() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 42.7;
        snapshot.gpu.temp_c = 71.0;
        snapshot.gpu.vram_used_mb = 4096;
        snapshot.gpu.vram_total_mb = 12288;

        assert_eq!(get_sensor_value("cpu.usage", &snapshot), "43");
        assert_eq!(get_sensor_value("gpu.temp", &snapshot), "71");
        assert_eq!(get_sensor_value("gpu.vram", &snapshot), "4096/12288");
        assert_eq!(get_sensor_value("gpu.vram.used", &snapshot), "4096");
    }

    #[test]
    fn nan_temp_returns_na() {
        let snapshot = SensorSnapshot::default();
        assert_eq!(get_sensor_value("cpu.temp", &snapshot), "N/A");
    }
}
