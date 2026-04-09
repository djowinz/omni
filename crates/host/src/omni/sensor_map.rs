//! Maps sensor path strings to SensorSnapshot field values.

use omni_shared::SensorSnapshot;

/// Returns `Some(())` if `path` is a known sensor path, `None` otherwise.
pub fn parse_sensor_path(path: &str) -> Option<()> {
    if path.starts_with("hwinfo.") && path.len() > 7 {
        return Some(());
    }
    match path {
        "cpu.usage" | "cpu.temp" | "gpu.usage" | "gpu.temp" | "gpu.clock" | "gpu.mem-clock"
        | "gpu.vram" | "gpu.vram.used" | "gpu.vram.total" | "gpu.power" | "gpu.fan"
        | "ram.usage" | "ram.used" | "ram.total" | "fps" | "frame-time" | "frame-time.avg"
        | "frame-time.1pct" | "frame-time.01pct" => Some(()),
        _ => None,
    }
}

/// Get the raw f64 value for a sensor path from a snapshot.
/// Returns None if the path is unknown or the value is unavailable.
fn get_raw_value(path: &str, snapshot: &SensorSnapshot) -> Option<f64> {
    match path {
        "cpu.usage" => Some(snapshot.cpu.total_usage_percent as f64),
        "cpu.temp" => nan_to_none(snapshot.cpu.package_temp_c as f64),
        "gpu.usage" => Some(snapshot.gpu.usage_percent as f64),
        "gpu.temp" => nan_to_none(snapshot.gpu.temp_c as f64),
        "gpu.clock" => Some(snapshot.gpu.core_clock_mhz as f64),
        "gpu.mem-clock" => Some(snapshot.gpu.mem_clock_mhz as f64),
        "gpu.vram.used" => Some(snapshot.gpu.vram_used_mb as f64),
        "gpu.vram.total" => Some(snapshot.gpu.vram_total_mb as f64),
        "gpu.power" => Some(snapshot.gpu.power_draw_w as f64),
        "gpu.fan" => Some(snapshot.gpu.fan_speed_percent as f64),
        "ram.usage" => Some(snapshot.ram.usage_percent as f64),
        "ram.used" => Some(snapshot.ram.used_mb as f64),
        "ram.total" => Some(snapshot.ram.total_mb as f64),
        "fps" if snapshot.frame.available => Some(snapshot.frame.fps as f64),
        "frame-time" if snapshot.frame.available => Some(snapshot.frame.frame_time_ms as f64),
        "frame-time.avg" if snapshot.frame.available => {
            Some(snapshot.frame.frame_time_avg_ms as f64)
        }
        "frame-time.1pct" if snapshot.frame.available => {
            Some(snapshot.frame.frame_time_1percent_ms as f64)
        }
        "frame-time.01pct" if snapshot.frame.available => {
            Some(snapshot.frame.frame_time_01percent_ms as f64)
        }
        _ => None,
    }
}

/// Default precision for a built-in sensor path.
fn default_precision(path: &str) -> usize {
    match path {
        "frame-time" | "frame-time.avg" | "frame-time.1pct" | "frame-time.01pct" => 1,
        _ => 0, // integers for temps, percentages, clocks, power, memory, fps
    }
}

/// Format a value with the given precision (decimal places).
fn format_with_precision(value: f64, precision: usize) -> String {
    format!("{:.prec$}", value, prec = precision)
}

fn nan_to_none(v: f64) -> Option<f64> {
    if v.is_nan() {
        None
    } else {
        Some(v)
    }
}

/// Get the formatted string value for a sensor path from a snapshot.
pub fn get_sensor_value(path: &str, snapshot: &SensorSnapshot) -> String {
    get_sensor_value_with_hwinfo(
        path,
        snapshot,
        &Default::default(),
        &Default::default(),
        None,
    )
}

/// Get the formatted string value for a sensor path, consulting HWiNFO data
/// for `hwinfo.*` paths and falling back to the snapshot for all other paths.
/// An optional `precision` overrides the default decimal places.
pub fn get_sensor_value_with_hwinfo(
    path: &str,
    snapshot: &SensorSnapshot,
    hwinfo_values: &std::collections::HashMap<String, f64>,
    hwinfo_units: &std::collections::HashMap<String, String>,
    precision: Option<usize>,
) -> String {
    // Special case: gpu.vram (composite format)
    if path == "gpu.vram" && precision.is_none() {
        return format!(
            "{}/{}",
            snapshot.gpu.vram_used_mb, snapshot.gpu.vram_total_mb
        );
    }

    if path.starts_with("hwinfo.") {
        return match hwinfo_values.get(path) {
            Some(&value) => {
                let unit = hwinfo_units.get(path).map(|s| s.as_str()).unwrap_or("");
                let prec = precision
                    .unwrap_or_else(|| crate::sensors::hwinfo::default_precision_for_unit(unit));
                format_with_precision(value, prec)
            }
            None => "N/A".to_string(),
        };
    }

    match get_raw_value(path, snapshot) {
        Some(value) => {
            let prec = precision.unwrap_or_else(|| default_precision(path));
            format_with_precision(value, prec)
        }
        None => "N/A".to_string(),
    }
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
            get_sensor_value_with_hwinfo(
                "hwinfo.cpu.core_0_temp",
                &snapshot,
                &hwinfo_values,
                &hwinfo_units,
                None
            ),
            "65"
        );
    }

    #[test]
    fn get_hwinfo_value_missing() {
        let snapshot = SensorSnapshot::default();
        let hwinfo_values = std::collections::HashMap::new();
        let hwinfo_units = std::collections::HashMap::new();
        assert_eq!(
            get_sensor_value_with_hwinfo(
                "hwinfo.cpu.core_0_temp",
                &snapshot,
                &hwinfo_values,
                &hwinfo_units,
                None
            ),
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
