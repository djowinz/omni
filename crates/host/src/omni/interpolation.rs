//! Interpolation of {sensor.path} expressions in text and style values.
//!
//! Scans a string for `{...}` patterns, looks up each path in the sensor map,
//! and replaces it with the current value.

use super::sensor_map;
use omni_shared::SensorSnapshot;

/// Replace all `{sensor.path}` expressions in the input string with current values.
#[allow(dead_code)]
pub fn interpolate(input: &str, snapshot: &SensorSnapshot) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            // Collect everything until '}'
            let mut path = String::new();
            let mut found_close = false;
            for inner in chars.by_ref() {
                if inner == '}' {
                    found_close = true;
                    break;
                }
                path.push(inner);
            }

            if found_close && !path.is_empty() {
                let value = sensor_map::get_sensor_value(path.trim(), snapshot);
                result.push_str(&value);
            } else {
                // Malformed — output as-is
                result.push('{');
                result.push_str(&path);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_simple() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 42.0;

        let result = interpolate("CPU: {cpu.usage}%", &snapshot);
        assert_eq!(result, "CPU: 42%");
    }

    #[test]
    fn interpolate_multiple() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 42.0;
        snapshot.gpu.temp_c = 71.0;

        let result = interpolate("{cpu.usage}% | {gpu.temp}°C", &snapshot);
        assert_eq!(result, "42% | 71°C");
    }

    #[test]
    fn interpolate_in_style_value() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.gpu.usage_percent = 83.0;

        let result = interpolate("width: {gpu.usage}%;", &snapshot);
        assert_eq!(result, "width: 83%;");
    }

    #[test]
    fn interpolate_unknown_path() {
        let snapshot = SensorSnapshot::default();
        let result = interpolate("{nonexistent}", &snapshot);
        assert_eq!(result, "N/A");
    }

    #[test]
    fn no_interpolation_passthrough() {
        let snapshot = SensorSnapshot::default();
        let result = interpolate("plain text", &snapshot);
        assert_eq!(result, "plain text");
    }
}
