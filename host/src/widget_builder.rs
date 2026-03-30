//! Builds ComputedWidget arrays from sensor snapshots.
//!
//! Phase 7: hardcoded sensor widget list.
//! Phase 9a: replaced by .widget file parsing + taffy layout engine.

use omni_shared::{ComputedWidget, SensorSnapshot, WidgetType, SensorSource, write_fixed_str};

/// Builds widgets from sensor data.
/// In Phase 9a, this gains a constructor that takes a parsed widget tree + theme.
pub struct WidgetBuilder;

impl WidgetBuilder {
    pub fn new() -> Self {
        Self
    }

    /// Build the widget array for one frame.
    /// In Phase 9a, this resolves styles, runs taffy layout, evaluates animations.
    pub fn build(&self, snapshot: &SensorSnapshot) -> Vec<ComputedWidget> {
        let mut widgets = Vec::new();
        let x = 20.0;
        let mut y = 20.0;
        let row_height = 28.0;
        let width = 260.0;

        // CPU Usage
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::CpuUsage,
            &format!("CPU: {:.0}%", snapshot.cpu.total_usage_percent),
        ));
        y += row_height;

        // CPU Temp
        let temp_text = if snapshot.cpu.package_temp_c.is_nan() {
            "CPU Temp: N/A".to_string()
        } else {
            format!("CPU Temp: {:.0}°C", snapshot.cpu.package_temp_c)
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::CpuTemp,
            &temp_text,
        ));
        y += row_height;

        // GPU Usage
        let gpu_text = if snapshot.gpu.usage_percent > 0.0 || snapshot.gpu.temp_c > 0.0 {
            format!("GPU: {:.0}%", snapshot.gpu.usage_percent)
        } else {
            "GPU: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuUsage,
            &gpu_text,
        ));
        y += row_height;

        // GPU Temp
        let gpu_temp_text = if snapshot.gpu.temp_c > 0.0 {
            format!("GPU Temp: {:.0}°C", snapshot.gpu.temp_c)
        } else {
            "GPU Temp: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuTemp,
            &gpu_temp_text,
        ));
        y += row_height;

        // GPU Clock
        let gpu_clock_text = if snapshot.gpu.core_clock_mhz > 0 {
            format!("GPU Clock: {} MHz", snapshot.gpu.core_clock_mhz)
        } else {
            "GPU Clock: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuClock,
            &gpu_clock_text,
        ));
        y += row_height;

        // VRAM
        let vram_text = if snapshot.gpu.vram_total_mb > 0 {
            format!("VRAM: {}/{} MB", snapshot.gpu.vram_used_mb, snapshot.gpu.vram_total_mb)
        } else {
            "VRAM: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuVram,
            &vram_text,
        ));
        y += row_height;

        // GPU Power
        let power_text = if snapshot.gpu.power_draw_w > 0.0 {
            format!("GPU Power: {:.0}W", snapshot.gpu.power_draw_w)
        } else {
            "GPU Power: N/A".to_string()
        };
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::GpuPower,
            &power_text,
        ));
        y += row_height;

        // RAM
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::RamUsage,
            &format!("RAM: {:.0}% ({}/{} MB)", snapshot.ram.usage_percent, snapshot.ram.used_mb, snapshot.ram.total_mb),
        ));
        y += row_height;

        // FPS (Phase 8 — always N/A for now)
        widgets.push(self.make_sensor_widget(
            x, y, width, row_height,
            SensorSource::Fps,
            "FPS: N/A",
        ));

        widgets
    }

    fn make_sensor_widget(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        source: SensorSource,
        text: &str,
    ) -> ComputedWidget {
        let mut widget = ComputedWidget::default();
        widget.widget_type = WidgetType::SensorValue;
        widget.source = source;
        widget.x = x;
        widget.y = y;
        widget.width = width;
        widget.height = height;
        widget.font_size = 16.0;
        widget.font_weight = 400;
        widget.color_rgba = [255, 255, 255, 255];
        widget.bg_color_rgba = [20, 20, 20, 180];
        widget.border_radius = [4.0; 4];
        widget.opacity = 1.0;
        write_fixed_str(&mut widget.format_pattern, text);
        widget
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_correct_number_of_widgets() {
        let builder = WidgetBuilder::new();
        let snapshot = SensorSnapshot::default();
        let widgets = builder.build(&snapshot);
        assert_eq!(widgets.len(), 9, "Should produce 9 sensor widgets");
    }

    #[test]
    fn widgets_are_vertically_stacked() {
        let builder = WidgetBuilder::new();
        let snapshot = SensorSnapshot::default();
        let widgets = builder.build(&snapshot);
        for i in 1..widgets.len() {
            assert!(widgets[i].y > widgets[i - 1].y,
                "Widget {} should be below widget {}", i, i - 1);
        }
    }

    #[test]
    fn unavailable_sensors_show_na() {
        let builder = WidgetBuilder::new();
        let snapshot = SensorSnapshot::default(); // all defaults — GPU zeroed, temp NaN
        let widgets = builder.build(&snapshot);

        // CPU temp (NaN) should show N/A
        let cpu_temp_text = omni_shared::read_fixed_str(&widgets[1].format_pattern);
        assert!(cpu_temp_text.contains("N/A"), "CPU temp should be N/A, got: {cpu_temp_text}");

        // GPU usage (0.0) should show N/A
        let gpu_text = omni_shared::read_fixed_str(&widgets[2].format_pattern);
        assert!(gpu_text.contains("N/A"), "GPU should be N/A, got: {gpu_text}");

        // FPS should always be N/A in this phase
        let fps_text = omni_shared::read_fixed_str(&widgets[8].format_pattern);
        assert!(fps_text.contains("N/A"), "FPS should be N/A, got: {fps_text}");
    }

    #[test]
    fn available_sensors_show_values() {
        let builder = WidgetBuilder::new();
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 42.0;
        snapshot.gpu.usage_percent = 83.0;
        snapshot.gpu.temp_c = 71.0;
        snapshot.ram.usage_percent = 62.0;
        snapshot.ram.used_mb = 16000;
        snapshot.ram.total_mb = 32000;

        let widgets = builder.build(&snapshot);

        let cpu_text = omni_shared::read_fixed_str(&widgets[0].format_pattern);
        assert!(cpu_text.contains("42"), "CPU should show 42%, got: {cpu_text}");

        let gpu_text = omni_shared::read_fixed_str(&widgets[2].format_pattern);
        assert!(gpu_text.contains("83"), "GPU should show 83%, got: {gpu_text}");

        let ram_text = omni_shared::read_fixed_str(&widgets[7].format_pattern);
        assert!(ram_text.contains("62"), "RAM should show 62%, got: {ram_text}");
    }
}
