/// Widget types shared between host and overlay DLL.
/// All types are #[repr(C)] for shared memory safety.

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ComputedWidget {
    pub widget_type: WidgetType,
    pub source: SensorSource,
    /// Absolute screen position in pixels.
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub font_weight: u16,
    pub color_rgba: [u8; 4],
    pub bg_color_rgba: [u8; 4],
    pub bg_gradient: GradientDef,
    pub border_color_rgba: [u8; 4],
    pub border_width: f32,
    /// Per-corner border radius: [top-left, top-right, bottom-right, bottom-left].
    pub border_radius: [f32; 4],
    pub opacity: f32,
    pub box_shadow: ShadowDef,
    /// Format pattern, e.g. "{value:.0f}°C". Null-terminated UTF-8.
    pub format_pattern: [u8; 128],
    /// Pre-formatted label text, e.g. "CPU". Null-terminated UTF-8.
    pub label_text: [u8; 64],
    /// Sensor value above this triggers critical state. f32::NAN to disable.
    pub critical_above: f32,
    pub critical_color_rgba: [u8; 4],
    /// For graph widgets: how many seconds of history to display.
    pub history_seconds: u32,
    /// For graph widgets: data point interval in milliseconds.
    pub history_interval_ms: u32,
    pub adaptive_color: AdaptiveColorMode,
    pub adaptive_light_rgba: [u8; 4],
    pub adaptive_dark_rgba: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetType {
    Label,
    SensorValue,
    Graph,
    Bar,
    Spacer,
    Group,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorSource {
    None,
    CpuUsage,
    CpuTemp,
    CpuFreqCore0,
    CpuFreqCore1,
    CpuFreqCore2,
    CpuFreqCore3,
    CpuFreqCore4,
    CpuFreqCore5,
    CpuFreqCore6,
    CpuFreqCore7,
    CpuFreqCore8,
    CpuFreqCore9,
    CpuFreqCore10,
    CpuFreqCore11,
    CpuFreqCore12,
    CpuFreqCore13,
    CpuFreqCore14,
    CpuFreqCore15,
    CpuFreqCore16,
    CpuFreqCore17,
    CpuFreqCore18,
    CpuFreqCore19,
    CpuFreqCore20,
    CpuFreqCore21,
    CpuFreqCore22,
    CpuFreqCore23,
    CpuFreqCore24,
    CpuFreqCore25,
    CpuFreqCore26,
    CpuFreqCore27,
    CpuFreqCore28,
    CpuFreqCore29,
    CpuFreqCore30,
    CpuFreqCore31,
    GpuUsage,
    GpuTemp,
    GpuClock,
    GpuMemClock,
    GpuVram,
    GpuPower,
    GpuFan,
    RamUsage,
    RamTemp,
    RamFreq,
    Fps,
    FrameTime,
    FrameTimeAvg,
    FrameTime1Pct,
    FrameTime01Pct,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdaptiveColorMode {
    Off,
    Auto,
    Custom,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct GradientDef {
    pub enabled: bool,
    pub angle_deg: f32,
    pub start_rgba: [u8; 4],
    pub end_rgba: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ShadowDef {
    pub enabled: bool,
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur_radius: f32,
    pub color_rgba: [u8; 4],
}

impl Default for ComputedWidget {
    fn default() -> Self {
        Self {
            widget_type: WidgetType::Label,
            source: SensorSource::None,
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            font_size: 13.0,
            font_weight: 400,
            color_rgba: [204, 204, 204, 255], // #cccccc
            bg_color_rgba: [0, 0, 0, 0],       // transparent
            bg_gradient: GradientDef::default(),
            border_color_rgba: [0, 0, 0, 0],
            border_width: 0.0,
            border_radius: [0.0; 4],
            opacity: 1.0,
            box_shadow: ShadowDef::default(),
            format_pattern: [0; 128],
            label_text: [0; 64],
            critical_above: f32::NAN,
            critical_color_rgba: [255, 68, 68, 255], // #ff4444
            history_seconds: 0,
            history_interval_ms: 0,
            adaptive_color: AdaptiveColorMode::Off,
            adaptive_light_rgba: [255, 255, 255, 255],
            adaptive_dark_rgba: [0, 0, 0, 255],
        }
    }
}

impl Default for GradientDef {
    fn default() -> Self {
        Self {
            enabled: false,
            angle_deg: 0.0,
            start_rgba: [0; 4],
            end_rgba: [0; 4],
        }
    }
}

impl Default for ShadowDef {
    fn default() -> Self {
        Self {
            enabled: false,
            offset_x: 0.0,
            offset_y: 0.0,
            blur_radius: 0.0,
            color_rgba: [0; 4],
        }
    }
}

/// Helper to write a string into a fixed-size null-terminated byte array.
/// Truncates if the string is too long.
pub fn write_fixed_str(dest: &mut [u8], src: &str) {
    let bytes = src.as_bytes();
    let max_len = dest.len() - 1; // reserve last byte for null terminator
    let copy_len = bytes.len().min(max_len);
    dest[..copy_len].copy_from_slice(&bytes[..copy_len]);
    // Null-terminate and zero remaining bytes
    dest[copy_len..].fill(0);
}

/// Helper to read a null-terminated UTF-8 string from a fixed-size byte array.
pub fn read_fixed_str(src: &[u8]) -> &str {
    let end = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    std::str::from_utf8(&src[..end]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn computed_widget_is_repr_c_sized() {
        let size = mem::size_of::<ComputedWidget>();
        assert!(size > 0);
    }

    #[test]
    fn widget_default_has_sane_values() {
        let w = ComputedWidget::default();
        assert_eq!(w.widget_type, WidgetType::Label);
        assert_eq!(w.source, SensorSource::None);
        assert_eq!(w.opacity, 1.0);
        assert_eq!(w.font_size, 13.0);
        assert!(w.critical_above.is_nan());
    }

    #[test]
    fn write_and_read_fixed_str() {
        let mut buf = [0u8; 64];
        write_fixed_str(&mut buf, "CPU Temp");
        assert_eq!(read_fixed_str(&buf), "CPU Temp");
    }

    #[test]
    fn write_fixed_str_truncates_long_input() {
        let mut buf = [0u8; 8];
        write_fixed_str(&mut buf, "This is a very long string");
        let result = read_fixed_str(&buf);
        assert_eq!(result.len(), 7); // 8 bytes - 1 null terminator
        assert_eq!(result, "This is");
    }

    #[test]
    fn sensor_source_all_cores_exist() {
        // Ensure we have all 32 core freq variants
        let _ = SensorSource::CpuFreqCore0;
        let _ = SensorSource::CpuFreqCore31;
    }
}
