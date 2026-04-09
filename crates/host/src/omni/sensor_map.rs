//! Maps sensor path strings to SensorSnapshot field values.

use omni_shared::SensorSnapshot;

/// Returns `Some(())` if `path` is a known sensor path, `None` otherwise.
pub fn parse_sensor_path(path: &str) -> Option<()> {
    if path.starts_with("hwinfo.") && path.len() > 7 {
        return Some(());
    }
    match path {
        "cpu.usage"
        | "cpu.temp"
        | "gpu.usage"
        | "gpu.temp"
        | "gpu.clock"
        | "gpu.mem-clock"
        | "gpu.vram"
        | "gpu.vram.used"
        | "gpu.vram.total"
        | "gpu.power"
        | "gpu.fan"
        | "ram.usage"
        | "ram.used"
        | "ram.total"
        | "fps"
        | "frame-time"
        | "frame-time.avg"
        | "frame-time.1pct"
        | "frame-time.01pct" => Some(()),
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

/// Get the formatted string value for a sensor path, consulting HWiNFO data
/// for `hwinfo.*` paths and falling back to the snapshot for all other paths.
pub fn get_sensor_value_with_hwinfo(
    path: &str,
    snapshot: &SensorSnapshot,
    hwinfo_values: &std::collections::HashMap<String, f64>,
    hwinfo_units: &std::collections::HashMap<String, String>,
) -> String {
    if path.starts_with("hwinfo.") {
        return match hwinfo_values.get(path) {
            Some(&value) => {
                let unit = hwinfo_units.get(path).map(|s| s.as_str()).unwrap_or("");
                crate::sensors::hwinfo::format_hwinfo_value(value, unit)
            }
            None => "N/A".to_string(),
        };
    }
    get_sensor_value(path, snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_paths() {
        assert!(parse_sensor_path("cpu.usage").is_some());
        assert!(parse_sensor_path("gpu.temp").is_some());
        assert!(parse_sensor_path("fps").is_some());
        assert!(parse_sensor_path("nonexistent").is_none());
    }

    #[test]
    fn parse_hwinfo_paths() {
        assert!(parse_sensor_path("hwinfo.cpu.core_0_temp").is_some());
        assert!(parse_sensor_path("hwinfo.gpu.gpu_temperature").is_some());
        assert!(parse_sensor_path("hwinfo.").is_none());
    }

    #[test]
    fn get_hwinfo_value_from_state() {
        let snapshot = SensorSnapshot::default();
        let mut hwinfo_values = std::collections::HashMap::new();
        hwinfo_values.insert("hwinfo.cpu.core_0_temp".to_string(), 65.0);
        let mut hwinfo_units = std::collections::HashMap::new();
        hwinfo_units.insert("hwinfo.cpu.core_0_temp".to_string(), "°C".to_string());
        assert_eq!(
            get_sensor_value_with_hwinfo("hwinfo.cpu.core_0_temp", &snapshot, &hwinfo_values, &hwinfo_units),
            "65"
        );
    }

    #[test]
    fn get_hwinfo_value_missing() {
        let snapshot = SensorSnapshot::default();
        let hwinfo_values = std::collections::HashMap::new();
        let hwinfo_units = std::collections::HashMap::new();
        assert_eq!(
            get_sensor_value_with_hwinfo("hwinfo.cpu.core_0_temp", &snapshot, &hwinfo_values, &hwinfo_units),
            "N/A"
        );
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
