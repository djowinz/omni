//! Unit-aware formatting and nice-tick algorithm for chart values.
//!
//! Units drive two things:
//! - How raw sensor values are formatted for display (e.g., `1500 bytes/s` → `1.5 KB/s`)
//! - How "nice" round-number bounds and tick values are selected for chart Y-axes

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    BytesPerSec,
    BitsPerSec,
    Bytes,
    Hz,
    Celsius,
    Percent,
    Count,
    Ms,
    Seconds,
    Rpm,
    Volts,
    Watts,
    Pascals,
    None,
}

impl Unit {
    /// Parse a unit identifier from a string. Used when the interpolation
    /// parser encounters a bare identifier in a Unit-typed argument position.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bytes/s" => Some(Unit::BytesPerSec),
            "bits/s" => Some(Unit::BitsPerSec),
            "bytes" => Some(Unit::Bytes),
            "Hz" => Some(Unit::Hz),
            "°C" => Some(Unit::Celsius),
            "%" => Some(Unit::Percent),
            "count" => Some(Unit::Count),
            "ms" => Some(Unit::Ms),
            "s" => Some(Unit::Seconds),
            "RPM" => Some(Unit::Rpm),
            "V" => Some(Unit::Volts),
            "W" => Some(Unit::Watts),
            "Pa" => Some(Unit::Pascals),
            "none" => Some(Unit::None),
            _ => None,
        }
    }

    /// Format a raw value for display in this unit.
    pub fn format(&self, value: f64) -> String {
        match self {
            Unit::BytesPerSec => format_scaled(value, 1024.0, BYTES_PREFIXES, "/s"),
            Unit::BitsPerSec => format_scaled(value, 1000.0, BITS_PREFIXES, "/s"),
            Unit::Bytes => format_scaled(value, 1024.0, BYTES_PREFIXES, ""),
            Unit::Hz => format_scaled(value, 1000.0, HZ_PREFIXES, ""),
            Unit::Celsius => format_simple(value, " °C"),
            Unit::Percent => format_simple(value, " %"),
            Unit::Count => format_count(value),
            Unit::Ms => format_simple(value, " ms"),
            Unit::Seconds => format_simple(value, " s"),
            Unit::Rpm => format_int(value, " RPM"),
            Unit::Volts => format_decimals(value, 2, " V"),
            Unit::Watts => format_int(value, " W"),
            Unit::Pascals => format_int(value, " Pa"),
            Unit::None => format_none(value),
        }
    }

    /// Compute nice round bounds that contain [min, max]. Uses Heckbert's
    /// algorithm: finds a "nice" step size, then rounds min down and max up
    /// to the nearest step multiple.
    pub fn nice_bounds(&self, min: f64, max: f64) -> (f64, f64) {
        if min == max {
            return (min - 1.0, max + 1.0);
        }
        let range = max - min;
        let step = nice_number(range / 4.0, true);
        let nice_min = (min / step).floor() * step;
        let nice_max = (max / step).ceil() * step;
        (nice_min, nice_max)
    }

    /// Compute tick values for a [min, max] range targeting approximately
    /// `target_count` ticks.
    pub fn nice_ticks(&self, min: f64, max: f64, target_count: usize) -> Vec<f64> {
        if min >= max || target_count == 0 {
            return vec![min, max];
        }
        let range = nice_number(max - min, false);
        let step = nice_number(
            range / (target_count.saturating_sub(1).max(1)) as f64,
            true,
        );
        let graph_min = (min / step).floor() * step;
        let graph_max = (max / step).ceil() * step;
        let mut ticks = Vec::new();
        let mut v = graph_min;
        let max_iter = (((graph_max - graph_min) / step) as usize).saturating_add(2);
        for _ in 0..max_iter {
            if v > graph_max + step * 1e-9 {
                break;
            }
            ticks.push(v);
            v += step;
        }
        ticks
    }
}

// ─── format helpers ───────────────────────────────────────────────────────

const BYTES_PREFIXES: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];
const BITS_PREFIXES: &[&str] = &["b", "Kb", "Mb", "Gb", "Tb", "Pb"];
const HZ_PREFIXES: &[&str] = &["Hz", "kHz", "MHz", "GHz", "THz"];
const COUNT_PREFIXES: &[&str] = &["", "K", "M", "B", "T"];

/// Format a value by walking up a prefix ladder until `value/scale < base`.
fn format_scaled(value: f64, base: f64, prefixes: &[&str], suffix: &str) -> String {
    if value == 0.0 {
        return format!("0 {}{}", prefixes[0], suffix);
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let mut abs = value.abs();
    let mut idx = 0;
    while abs >= base && idx < prefixes.len() - 1 {
        abs /= base;
        idx += 1;
    }
    if idx == 0 {
        format!("{}{} {}{}", sign, abs as i64, prefixes[idx], suffix)
    } else if abs >= 100.0 {
        format!("{}{} {}{}", sign, abs as i64, prefixes[idx], suffix)
    } else {
        format!("{}{:.1} {}{}", sign, abs, prefixes[idx], suffix)
    }
}

/// Format a count with decimal scaling but no base unit label.
fn format_count(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let mut abs = value.abs();
    let mut idx = 0;
    while abs >= 1000.0 && idx < COUNT_PREFIXES.len() - 1 {
        abs /= 1000.0;
        idx += 1;
    }
    if idx == 0 {
        format!("{}{}", sign, abs as i64)
    } else if abs >= 100.0 {
        format!("{}{}{}", sign, abs as i64, COUNT_PREFIXES[idx])
    } else {
        format!("{}{:.1}{}", sign, abs, COUNT_PREFIXES[idx])
    }
}

/// Format a plain value with a suffix. Uses 1 decimal if fractional part is
/// significant (abs value < 100 or value is not whole). Zero and whole numbers
/// render as integers.
fn format_simple(value: f64, suffix: &str) -> String {
    if value == 0.0 {
        return format!("0{}", suffix);
    }
    if value == value.trunc() {
        return format!("{}{}", value as i64, suffix);
    }
    if value.abs() >= 100.0 {
        // Round to nearest integer rather than truncate, but still show as int
        format!("{}{}", value.round() as i64, suffix)
    } else {
        format!("{:.1}{}", value, suffix)
    }
}

/// Format as an integer with a suffix.
fn format_int(value: f64, suffix: &str) -> String {
    format!("{}{}", value as i64, suffix)
}

/// Format with a fixed number of decimal places and a suffix.
fn format_decimals(value: f64, decimals: usize, suffix: &str) -> String {
    format!("{:.*}{}", decimals, value, suffix)
}

/// Format a unitless value, preserving fractional digits as-is (no truncation).
fn format_none(value: f64) -> String {
    if value == value.trunc() {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

/// Heckbert's "Nice Numbers for Graph Labels" (Graphics Gems, 1990).
fn nice_number(x: f64, round: bool) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    let exp = x.log10().floor();
    let f = x / 10f64.powf(exp);
    let nf = if round {
        if f < 1.5 {
            1.0
        } else if f < 3.0 {
            2.0
        } else if f < 7.0 {
            5.0
        } else {
            10.0
        }
    } else if f <= 1.0 {
        1.0
    } else if f <= 2.0 {
        2.0
    } else if f <= 5.0 {
        5.0
    } else {
        10.0
    };
    nf * 10f64.powf(exp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_all_units() {
        assert_eq!(Unit::parse("bytes/s"), Some(Unit::BytesPerSec));
        assert_eq!(Unit::parse("bits/s"), Some(Unit::BitsPerSec));
        assert_eq!(Unit::parse("bytes"), Some(Unit::Bytes));
        assert_eq!(Unit::parse("Hz"), Some(Unit::Hz));
        assert_eq!(Unit::parse("°C"), Some(Unit::Celsius));
        assert_eq!(Unit::parse("%"), Some(Unit::Percent));
        assert_eq!(Unit::parse("count"), Some(Unit::Count));
        assert_eq!(Unit::parse("ms"), Some(Unit::Ms));
        assert_eq!(Unit::parse("s"), Some(Unit::Seconds));
        assert_eq!(Unit::parse("RPM"), Some(Unit::Rpm));
        assert_eq!(Unit::parse("V"), Some(Unit::Volts));
        assert_eq!(Unit::parse("W"), Some(Unit::Watts));
        assert_eq!(Unit::parse("Pa"), Some(Unit::Pascals));
        assert_eq!(Unit::parse("none"), Some(Unit::None));
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(Unit::parse("fubar"), None);
        assert_eq!(Unit::parse(""), None);
    }

    #[test]
    fn format_bytes_scales_binary() {
        assert_eq!(Unit::Bytes.format(0.0), "0 B");
        assert_eq!(Unit::Bytes.format(500.0), "500 B");
        assert_eq!(Unit::Bytes.format(1024.0), "1.0 KB");
        assert_eq!(Unit::Bytes.format(1500.0), "1.5 KB");
        assert_eq!(Unit::Bytes.format(1_048_576.0), "1.0 MB");
        assert_eq!(Unit::Bytes.format(100_000_000.0), "95.4 MB");
        assert_eq!(Unit::Bytes.format(1_500_000_000.0), "1.4 GB");
        assert_eq!(Unit::Bytes.format(1_500_000_000_000.0), "1.4 TB");
    }

    #[test]
    fn format_bytes_per_sec() {
        assert_eq!(Unit::BytesPerSec.format(0.0), "0 B/s");
        assert_eq!(Unit::BytesPerSec.format(1500.0), "1.5 KB/s");
        assert_eq!(Unit::BytesPerSec.format(100_000_000.0), "95.4 MB/s");
    }

    #[test]
    fn format_bits_per_sec_decimal() {
        assert_eq!(Unit::BitsPerSec.format(0.0), "0 b/s");
        assert_eq!(Unit::BitsPerSec.format(1000.0), "1.0 Kb/s");
        assert_eq!(Unit::BitsPerSec.format(1_000_000.0), "1.0 Mb/s");
        assert_eq!(Unit::BitsPerSec.format(1_500_000_000.0), "1.5 Gb/s");
    }

    #[test]
    fn format_hz_decimal() {
        assert_eq!(Unit::Hz.format(0.0), "0 Hz");
        assert_eq!(Unit::Hz.format(1000.0), "1.0 kHz");
        assert_eq!(Unit::Hz.format(3_600_000_000.0), "3.6 GHz");
    }

    #[test]
    fn format_celsius() {
        assert_eq!(Unit::Celsius.format(0.0), "0 °C");
        assert_eq!(Unit::Celsius.format(72.5), "72.5 °C");
        assert_eq!(Unit::Celsius.format(-10.0), "-10 °C");
    }

    #[test]
    fn format_percent() {
        assert_eq!(Unit::Percent.format(0.0), "0 %");
        assert_eq!(Unit::Percent.format(85.0), "85 %");
        assert_eq!(Unit::Percent.format(99.9), "99.9 %");
    }

    #[test]
    fn format_count_decimal() {
        assert_eq!(Unit::Count.format(0.0), "0");
        assert_eq!(Unit::Count.format(500.0), "500");
        assert_eq!(Unit::Count.format(12345.0), "12.3K");
        assert_eq!(Unit::Count.format(1_500_000.0), "1.5M");
    }

    #[test]
    fn format_ms_seconds() {
        assert_eq!(Unit::Ms.format(16.7), "16.7 ms");
        assert_eq!(Unit::Seconds.format(2.5), "2.5 s");
    }

    #[test]
    fn format_rpm_volts_watts_pascals() {
        assert_eq!(Unit::Rpm.format(2400.0), "2400 RPM");
        assert_eq!(Unit::Volts.format(1.23), "1.23 V");
        assert_eq!(Unit::Watts.format(145.0), "145 W");
        assert_eq!(Unit::Pascals.format(1013.0), "1013 Pa");
    }

    #[test]
    fn format_none_is_plain() {
        assert_eq!(Unit::None.format(0.0), "0");
        assert_eq!(Unit::None.format(1234.5), "1234.5");
    }

    #[test]
    fn format_negative_bytes_preserves_sign() {
        assert_eq!(Unit::BytesPerSec.format(-1500.0), "-1.5 KB/s");
    }

    #[test]
    fn nice_bounds_simple() {
        let (min, max) = Unit::Percent.nice_bounds(0.0, 85.0);
        assert_eq!(min, 0.0);
        assert_eq!(max, 100.0);
    }

    #[test]
    fn nice_bounds_network() {
        let (min, max) = Unit::BytesPerSec.nice_bounds(0.0, 1_234_567.0);
        assert_eq!(min, 0.0);
        assert!(max >= 1_234_567.0);
        assert!(max <= 2_500_000.0);
    }

    #[test]
    fn nice_bounds_tight_range() {
        let (min, max) = Unit::Celsius.nice_bounds(72.0, 78.0);
        assert!(min <= 72.0);
        assert!(max >= 78.0);
    }

    #[test]
    fn nice_ticks_returns_round_values() {
        let ticks = Unit::Percent.nice_ticks(0.0, 100.0, 5);
        assert!(ticks.contains(&0.0));
        assert!(ticks.contains(&100.0));
        assert!(ticks.len() >= 4 && ticks.len() <= 6);
    }

    #[test]
    fn nice_ticks_small_range() {
        let ticks = Unit::Ms.nice_ticks(15.0, 17.0, 4);
        assert!(ticks.len() >= 2);
        assert!(ticks[0] <= 15.0);
        assert!(*ticks.last().unwrap() >= 17.0);
    }
}
