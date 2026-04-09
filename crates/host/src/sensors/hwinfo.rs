use std::collections::HashMap;

use tracing::{info, warn};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Memory::{
    MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ,
};

// ──────────────────────────────────────────────────────────────────────────────
// HWiNFO shared-memory binary layout
// Documented at: https://www.hwinfo.com/forum/threads/shared-memory-support.1679/
// ──────────────────────────────────────────────────────────────────────────────

const HWINFO_SM_NAME_GLOBAL: &str = "Global\\HWiNFO_SENS_SM2";
const HWINFO_SM_NAME_LOCAL: &str = "HWiNFO_SENS_SM2";
const HWINFO_MEM_VERSION: u32 = 2;

/// Maximum string lengths as defined by HWiNFO's shared memory spec.
const HWINFO_SENSOR_STRING_LEN: usize = 128;
const HWINFO_READING_STRING_LEN: usize = 128;
const HWINFO_UNIT_STRING_LEN: usize = 16;

/// Top-level header at offset 0 of the shared memory block.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Fields read via pointer cast from shared memory
pub struct HwInfoHeader {
    /// Signature: "HWiS" (0x53695748).
    pub signature: u32,
    /// Must equal 2 for HWiNFO-SM2 format.
    pub version: u32,
    pub revision: u32,
    pub poll_time_lo: u32,
    pub poll_time_hi: u32,
    pub sensor_section_offset: u32,
    pub sensor_size: u32,
    pub sensor_count: u32,
    pub reading_section_offset: u32,
    pub reading_size: u32,
    pub reading_count: u32,
}

/// One sensor device entry (e.g. "CPU [#0]: AMD Ryzen 9 7950X").
#[repr(C)]
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Fields read via pointer cast from shared memory
pub struct HwInfoSensor {
    pub id: u32,
    pub instance: u32,
    pub name_original: [u8; HWINFO_SENSOR_STRING_LEN],
    pub name_user: [u8; HWINFO_SENSOR_STRING_LEN],
}

/// Reading type enum matching HWiNFO's SENSOR_READING_TYPE.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Variants matched via raw u32 from shared memory
pub enum ReadingType {
    None = 0,
    Temp = 1,
    Voltage = 2,
    Fan = 3,
    Current = 4,
    Power = 5,
    Clock = 6,
    Usage = 7,
    Other = 8,
}

/// One reading entry (e.g. "Core 0 Temperature").
/// Packed to match HWiNFO's memory layout exactly — no padding between unit and value.
#[repr(C, packed)]
#[derive(Clone, Copy)]
#[allow(dead_code)] // Fields read via pointer cast from shared memory
pub struct HwInfoReading {
    pub reading_type: u32,
    pub sensor_index: u32,
    pub id: u32,
    pub label_original: [u8; HWINFO_READING_STRING_LEN],
    pub label_user: [u8; HWINFO_READING_STRING_LEN],
    pub unit: [u8; HWINFO_UNIT_STRING_LEN],
    pub value: f64,
    pub value_min: f64,
    pub value_max: f64,
    pub value_avg: f64,
}

// ──────────────────────────────────────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────────────────────────────────────

/// Metadata describing a single sensor reading.
#[derive(Debug, Clone)]
pub struct HwInfoSensorMeta {
    /// Dot-separated path like `hwinfo.cpu.core_0_temperature`.
    pub path: String,
    /// Human-readable label (original from HWiNFO).
    pub label: String,
    /// Unit string (e.g. "°C", "%", "MHz").
    pub unit: String,
}

/// Full state snapshot from HWiNFO shared memory.
#[derive(Debug, Default, Clone)]
pub struct HwInfoState {
    pub connected: bool,
    pub sensor_count: u32,
    /// Map from `path` → raw f64 value.
    pub values: HashMap<String, f64>,
    /// Ordered list of sensor metadata (one per reading).
    pub sensors: Vec<HwInfoSensorMeta>,
    /// Map from `path` → unit string.
    pub units: HashMap<String, String>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Path helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Build a normalised dot-path: `hwinfo.<category>.<reading_label>`.
///
/// - `sensor_name`: the raw sensor name from HWiNFO (e.g. `"CPU [#0]: AMD Ryzen 9 7950X"`)
/// - `reading_label`: the raw reading label (e.g. `"Core 0 Temperature"`)
pub fn normalize_label(sensor_name: &str, reading_label: &str) -> String {
    let category = extract_category(sensor_name);
    let label = sanitize_segment(reading_label);
    format!("hwinfo.{}.{}", category, label)
}

/// Extract a short, lowercase category from a sensor name.
///
/// Rules (applied in order):
/// 1. Strip ` [#N]: …` suffix (everything from ` [#` onward).
/// 2. Strip `: …` suffix (everything after first `:`).
/// 3. Keep only the first "word" before any whitespace.
/// 4. Lowercase it.
fn extract_category(sensor_name: &str) -> String {
    // Strip ` [#N]: ...` — common for CPU/GPU entries
    let s = if let Some(idx) = sensor_name.find(" [#") {
        &sensor_name[..idx]
    } else {
        sensor_name
    };

    // Strip `: ...` — common for Drive entries
    let s = if let Some(idx) = s.find(':') {
        s[..idx].trim()
    } else {
        s.trim()
    };

    // Take the first word only (e.g. "GPU" from "GPU Temperature")
    let word = s.split_whitespace().next().unwrap_or(s);
    sanitize_segment(word)
}

/// Sanitise a path segment: lowercase, spaces/dashes/slashes/dots → `_`,
/// strip all other non-alphanumeric-or-underscore chars, collapse `__+` → `_`,
/// trim leading/trailing `_`.
fn sanitize_segment(s: &str) -> String {
    let lower = s.to_lowercase();
    let mut out = String::with_capacity(lower.len());

    for ch in lower.chars() {
        match ch {
            ' ' | '-' | '/' | '.' | '(' | ')' | '[' | ']' => out.push('_'),
            c if c.is_alphanumeric() || c == '_' => out.push(c),
            _ => {} // strip other special chars like `°`, `#`, etc.
        }
    }

    // Collapse consecutive underscores
    let mut result = String::with_capacity(out.len());
    let mut prev_under = false;
    for ch in out.chars() {
        if ch == '_' {
            if !prev_under {
                result.push(ch);
            }
            prev_under = true;
        } else {
            result.push(ch);
            prev_under = false;
        }
    }

    // Trim leading/trailing underscores
    result.trim_matches('_').to_string()
}

/// Append `_2`, `_3`, … to `path` if it has already been seen.
///
/// `seen` maps base path → how many times it has been encountered.
pub fn dedup_path(path: &str, seen: &mut HashMap<String, u32>) -> String {
    let count = seen.entry(path.to_string()).or_insert(0);
    *count += 1;
    if *count == 1 {
        path.to_string()
    } else {
        format!("{}_{}", path, count)
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Value formatting
// ──────────────────────────────────────────────────────────────────────────────

/// Format a reading value according to its unit.
///
/// | Unit      | Format            |
/// |-----------|-------------------|
/// | °C        | integer           |
/// | %         | integer           |
/// | V         | 2 decimal places  |
/// | MHz       | integer           |
/// | W         | integer           |
/// | RPM       | 1 decimal place   |
/// | (other)   | 1 decimal place   |
/// Default decimal places for a HWiNFO reading based on its unit.
pub fn default_precision_for_unit(unit: &str) -> usize {
    match unit {
        "°C" | "°F" | "%" | "MHz" | "W" | "RPM" | "MB" | "GB" | "KB/s" | "MB/s" => 0,
        "V" | "A" => 2,
        _ => 1,
    }
}

#[allow(dead_code)] // Used in tests; public API for future formatting needs
pub fn format_hwinfo_value(value: f64, unit: &str) -> String {
    let prec = default_precision_for_unit(unit);
    format!("{:.prec$}", value, prec = prec)
}

// ──────────────────────────────────────────────────────────────────────────────
// Shared memory reader
// ──────────────────────────────────────────────────────────────────────────────

/// Polls HWiNFO shared memory and maintains sensor state.
pub struct HwInfoReader {
    state: HwInfoState,
    /// True when we logged "HWiNFO detected".
    logged_connect: bool,
    /// True when we logged "HWiNFO disconnected" (or never connected).
    logged_disconnect: bool,
    /// Reported version mismatch once.
    logged_version_mismatch: bool,
}

impl HwInfoReader {
    pub fn new() -> Self {
        Self {
            state: HwInfoState::default(),
            logged_connect: false,
            logged_disconnect: false,
            logged_version_mismatch: false,
        }
    }

    /// Poll HWiNFO shared memory.
    ///
    /// Returns `(&HwInfoState, bool)` where the bool is `true` when the sensor
    /// list has changed since the last call (new connection, disconnection, or
    /// different reading count).
    pub fn poll(&mut self) -> (&HwInfoState, bool) {
        match self.try_read() {
            Ok((new_state, changed)) => {
                if new_state.connected && !self.logged_connect {
                    info!(
                        sensor_count = new_state.sensor_count,
                        "HWiNFO detected ({} sensors)", new_state.sensor_count
                    );
                    self.logged_connect = true;
                    self.logged_disconnect = false;
                }
                self.state = new_state;
                (&self.state, changed)
            }
            Err(_) => {
                let was_connected = self.state.connected;
                if was_connected && !self.logged_disconnect {
                    info!("HWiNFO disconnected");
                    self.logged_disconnect = true;
                    self.logged_connect = false;
                }
                let changed = was_connected; // transition to disconnected
                self.state = HwInfoState::default();
                (&self.state, changed)
            }
        }
    }

    fn try_read(&mut self) -> Result<(HwInfoState, bool), ()> {
        // Try Global\ namespace first, then session-local.
        let handle = open_mapping(HWINFO_SM_NAME_GLOBAL)
            .or_else(|| open_mapping(HWINFO_SM_NAME_LOCAL))
            .ok_or(())?;

        let result = unsafe { self.read_mapping(handle) };

        // Always close the handle before returning.
        unsafe {
            windows::Win32::Foundation::CloseHandle(handle).ok();
        }

        result
    }

    unsafe fn read_mapping(&mut self, handle: HANDLE) -> Result<(HwInfoState, bool), ()> {
        let base = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0);
        if base.Value.is_null() {
            return Err(());
        }

        let result = self.parse_view(base.Value as *const u8);

        UnmapViewOfFile(base).ok();

        result
    }

    unsafe fn parse_view(&mut self, base: *const u8) -> Result<(HwInfoState, bool), ()> {
        // Read the header (use read_unaligned — shared memory may not be aligned).
        let header = std::ptr::read_unaligned(base as *const HwInfoHeader);

        if header.version != HWINFO_MEM_VERSION {
            if !self.logged_version_mismatch {
                warn!(
                    version = header.version,
                    expected = HWINFO_MEM_VERSION,
                    "HWiNFO shared memory version mismatch — expected v{}, got v{}",
                    HWINFO_MEM_VERSION,
                    header.version
                );
                self.logged_version_mismatch = true;
            }
            return Err(());
        }

        let sensor_count = header.sensor_count;
        let reading_count = header.reading_count;

        // Build sensor name lookup: sensor_index → raw name string
        let mut sensor_names: Vec<String> = Vec::with_capacity(sensor_count as usize);
        for i in 0..sensor_count {
            let offset =
                header.sensor_section_offset as usize + i as usize * header.sensor_size as usize;
            let sensor = std::ptr::read_unaligned(base.add(offset) as *const HwInfoSensor);
            let name = c_str_to_string(&sensor.name_user);
            let name = if name.is_empty() {
                c_str_to_string(&sensor.name_original)
            } else {
                name
            };
            sensor_names.push(name);
        }

        // Walk readings, build state.
        let mut values = HashMap::new();
        let mut sensors = Vec::with_capacity(reading_count as usize);
        let mut units = HashMap::new();
        let mut seen_paths: HashMap<String, u32> = HashMap::new();

        for i in 0..reading_count {
            let reading_base =
                header.reading_section_offset as usize + i as usize * header.reading_size as usize;
            let reading = std::ptr::read_unaligned(base.add(reading_base) as *const HwInfoReading);

            let si = reading.sensor_index as usize;
            let sensor_name = sensor_names.get(si).map(|s| s.as_str()).unwrap_or("");

            let label_user = c_str_to_string(&reading.label_user);
            let label = if label_user.is_empty() {
                c_str_to_string(&reading.label_original)
            } else {
                label_user
            };
            let unit = c_str_to_string(&reading.unit);

            if label.is_empty() {
                continue;
            }

            let raw_path = normalize_label(sensor_name, &label);
            let path = dedup_path(&raw_path, &mut seen_paths);

            values.insert(path.clone(), reading.value);
            units.insert(path.clone(), unit.clone());
            sensors.push(HwInfoSensorMeta {
                path,
                label: format!("{} — {}", sensor_name, label),
                unit,
            });
        }

        // Only keep sensors that have a value entry (dedup may cause Vec/HashMap mismatch)
        let sensors: Vec<HwInfoSensorMeta> = sensors
            .into_iter()
            .filter(|s| values.contains_key(&s.path))
            .collect();

        let new_state = HwInfoState {
            connected: true,
            sensor_count: sensors.len() as u32,
            values,
            sensors,
            units,
        };

        // Detect whether the sensor list changed.
        let changed = !self.state.connected
            || self.state.sensor_count != new_state.sensor_count
            || self.state.sensors.len() != new_state.sensors.len();

        Ok((new_state, changed))
    }
}

impl Default for HwInfoReader {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Windows helpers
// ──────────────────────────────────────────────────────────────────────────────

fn open_mapping(name: &str) -> Option<HANDLE> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(wide.as_ptr())) };
    match handle {
        Ok(h) if !h.is_invalid() => Some(h),
        _ => None,
    }
}

/// Convert a null-terminated byte slice to a UTF-8 `String`, lossy.
fn c_str_to_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_basic_label() {
        let path = normalize_label("CPU", "Core 0 Temperature");
        assert_eq!(path, "hwinfo.cpu.core_0_temperature");
    }

    #[test]
    fn normalize_strips_special_chars() {
        let path = normalize_label("CPU [#0]: AMD Ryzen 9", "Core 0 T(Tctl/Tdie)");
        assert_eq!(path, "hwinfo.cpu.core_0_t_tctl_tdie");
    }

    #[test]
    fn normalize_strips_sensor_index() {
        let path = normalize_label("GPU [#0]: NVIDIA RTX 4090", "GPU Temperature");
        assert_eq!(path, "hwinfo.gpu.gpu_temperature");
    }

    #[test]
    fn normalize_handles_drive_labels() {
        let path = normalize_label("Drive: Samsung 990 Pro", "Drive Temperature");
        assert_eq!(path, "hwinfo.drive.drive_temperature");
    }

    #[test]
    fn dedup_duplicate_labels() {
        let mut seen = HashMap::new();
        let p1 = dedup_path("hwinfo.cpu.core_temp", &mut seen);
        let p2 = dedup_path("hwinfo.cpu.core_temp", &mut seen);
        let p3 = dedup_path("hwinfo.cpu.core_temp", &mut seen);
        assert_eq!(p1, "hwinfo.cpu.core_temp");
        assert_eq!(p2, "hwinfo.cpu.core_temp_2");
        assert_eq!(p3, "hwinfo.cpu.core_temp_3");
    }

    #[test]
    fn format_value_by_unit() {
        // °C and % → integer
        assert_eq!(format_hwinfo_value(72.9, "°C"), "73");
        assert_eq!(format_hwinfo_value(45.6, "%"), "46");
        // MHz and W → integer
        assert_eq!(format_hwinfo_value(3600.7, "MHz"), "3601");
        assert_eq!(format_hwinfo_value(125.4, "W"), "125");
        // V → 2 decimal places
        assert_eq!(format_hwinfo_value(1.234, "V"), "1.23");
        // RPM → 1 decimal place
        assert_eq!(format_hwinfo_value(1234.5, "RPM"), "1234.5");
    }
}
