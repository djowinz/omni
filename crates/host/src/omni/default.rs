//! Built-in default .omni content.
//! Replicates the hardcoded sensor dashboard from the WidgetBuilder.

pub const DEFAULT_OMNI: &str = r#"
<widget id="system-stats" name="System Stats" enabled="true">
  <template>
    <div class="panel" style="position: fixed; top: 20px; left: 20px;">
      <span class="val">CPU: {cpu.usage}%</span>
      <span class="val">CPU Temp: {cpu.temp}°C</span>
      <span class="val">GPU: {gpu.usage}%</span>
      <span class="val">GPU Temp: {gpu.temp}°C</span>
      <span class="val">GPU Clock: {gpu.clock} MHz</span>
      <span class="val">VRAM: {gpu.vram.used}/{gpu.vram.total} MB</span>
      <span class="val">GPU Power: {gpu.power}W</span>
      <span class="val">GPU Fan: {gpu.fan}%</span>
      <span class="val">RAM: {ram.usage}%</span>
      <span class="val">FPS: {fps}</span>
    </div>
  </template>
  <style>
    .panel {
      background: rgba(20, 20, 20, 0.7);
      border-radius: 4px;
      padding: 6px;
      display: flex;
      flex-direction: column;
      gap: 2px;
    }
    .val {
      color: #ffffff;
      font-size: 16px;
      font-weight: 400;
    }
  </style>
</widget>
"#;

pub const DEFAULT_THEME_CSS: &str = r#":root {
  --bg: rgba(20, 20, 20, 0.7);
  --bg-light: rgba(40, 40, 40, 0.6);
  --text: #ffffff;
  --text-dim: #aaaaaa;
  --accent: #44ff88;
  --warning: #ff8844;
  --critical: #ff4444;
  --font: 'Segoe UI';
  --font-size: 14px;
}
"#;

#[cfg(test)]
mod tests {
    use super::super::parser;
    use super::*;

    #[test]
    fn default_omni_parses() {
        let file = parser::parse_omni(DEFAULT_OMNI).unwrap();
        assert_eq!(file.widgets.len(), 1);
        assert_eq!(file.widgets[0].id, "system-stats");
        assert!(file.widgets[0].enabled);
    }

    #[test]
    fn default_overlay_lowers_to_data_sensor_spans() {
        use crate::omni::history::SensorHistory;
        use crate::omni::html_builder;
        use crate::omni::view_trust::ViewTrust;
        use omni_shared::SensorSnapshot;
        use std::collections::HashMap;

        let file = parser::parse_omni(DEFAULT_OMNI).unwrap();
        let snap = SensorSnapshot::default();
        let hv: HashMap<String, f64> = HashMap::new();
        let hu: HashMap<String, String> = HashMap::new();
        let history = SensorHistory::new();
        let rendered = html_builder::build_initial_html(
            &file,
            &snap,
            1920,
            1080,
            std::path::Path::new("."),
            "default",
            &hv,
            &hu,
            &history,
            ViewTrust::LocalAuthored,
        );
        assert!(rendered.html.contains(r#"data-sensor="cpu.usage""#));
        assert!(rendered.html.contains(r#"data-sensor="gpu.temp""#));
        assert!(rendered.html.contains(r#"data-sensor-format="percent""#));
        assert!(rendered
            .html
            .contains(r#"data-sensor-format="temperature""#));
    }
}
