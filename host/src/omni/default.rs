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
      background: rgba(20, 20, 20, 180);
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

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::parser;

    #[test]
    fn default_omni_parses() {
        let file = parser::parse_omni(DEFAULT_OMNI).unwrap();
        assert_eq!(file.widgets.len(), 1);
        assert_eq!(file.widgets[0].id, "system-stats");
        assert!(file.widgets[0].enabled);
    }
}
