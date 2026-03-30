# Phase 9a-1: Core Widget Format + Parser

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `.omni` file format parser and resolver so users can define overlay layouts using HTML+CSS with sensor data interpolation, replacing the hardcoded `WidgetBuilder`.

**Architecture:** A `.omni` file contains `<theme>` directive, `<widget>` blocks (each with `<template>` + `<style>`), parsed by `quick-xml` into an `OmniFile` data structure. `lightningcss` parses CSS. An `OmniResolver` walks the HTML tree, resolves CSS properties (theme variables → scoped styles → inline styles), interpolates `{sensor.path}` expressions, and emits `Vec<ComputedWidget>` for the existing shared memory pipeline. WebSocket endpoints `widget.parse` and `widget.update` enable future Electron integration.

**Tech Stack:** Rust, `quick-xml` (XML parsing), `lightningcss` (CSS parsing), `serde`/`serde_json` (JSON serialization for WebSocket).

**Testing notes:** Parser and resolver are fully unit-testable with `.omni` source strings. CSS resolution testable with known inputs. Manual test: create `.omni` file, see it rendered in-game.

**Depends on:** Phase 9a-0 complete (WebSocket server, service mode).

---

## File Map

```
host/
  Cargo.toml                         # Add quick-xml, lightningcss
  src/
    main.rs                          # Replace widget_builder with omni_resolver, load .omni on startup
    widget_builder.rs                # DELETE (replaced by omni module)
    ws_server.rs                     # Add widget.parse and widget.update handlers
    omni/
      mod.rs                         # Public API: parse_omni_file, OmniFile, OmniResolver
      types.rs                       # OmniFile, Widget, HtmlNode, StyleProperty — JSON-serializable
      parser.rs                      # quick-xml parser: .omni source → OmniFile
      css.rs                         # lightningcss integration: parse CSS, resolve properties
      resolver.rs                    # OmniResolver: (OmniFile, SensorSnapshot) → Vec<ComputedWidget>
      interpolation.rs               # {sensor.path} interpolation in text and style values
      sensor_map.rs                  # Maps "cpu.usage" strings to SensorSource + current values
      default.rs                     # Built-in default .omni content (replaces hardcoded widgets)
```

---

### Task 1: Add Dependencies

**Files:**
- Modify: `host/Cargo.toml`

- [ ] **Step 1: Add quick-xml and lightningcss**

```toml
[dependencies]
omni-shared = { path = "../shared" }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
ctrlc = "3"
sysinfo = "0.35"
wmi = "0.14"
tungstenite = "0.26"
quick-xml = "0.37"
lightningcss = "1.0"
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Downloads new crates, compiles.

Note: `lightningcss` may require a specific version. If `1.0` doesn't exist, use the latest available (e.g., `1.0.0-alpha.65` or similar). Check crates.io for the current version.

- [ ] **Step 3: Commit**

```bash
git add host/Cargo.toml Cargo.lock
git commit -m "feat(host): add quick-xml and lightningcss for .omni file parsing"
```

---

### Task 2: Core Data Types

**Files:**
- Create: `host/src/omni/types.rs`
- Create: `host/src/omni/mod.rs`
- Modify: `host/src/main.rs` (add `mod omni;`)

These are the JSON-serializable data structures that represent a parsed `.omni` file.

- [ ] **Step 1: Create host/src/omni/types.rs**

```rust
//! Data types for the parsed .omni file format.
//! All types are JSON-serializable for Electron WebSocket communication.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A parsed .omni file containing a theme reference and widget definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniFile {
    /// Optional path to a theme CSS file.
    pub theme_src: Option<String>,
    /// Ordered list of widget definitions.
    pub widgets: Vec<Widget>,
}

/// A single widget definition with its template and scoped styles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Widget {
    /// Unique identifier (required).
    pub id: String,
    /// Human-readable display name (required).
    pub name: String,
    /// Whether this widget is rendered.
    pub enabled: bool,
    /// Root of the HTML template tree.
    pub template: HtmlNode,
    /// CSS rules scoped to this widget.
    pub style_source: String,
}

/// A node in the HTML template tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum HtmlNode {
    Element {
        tag: String,
        id: Option<String>,
        classes: Vec<String>,
        /// Inline style attribute value (unparsed CSS).
        inline_style: Option<String>,
        children: Vec<HtmlNode>,
    },
    Text {
        content: String,
    },
}

/// A resolved CSS property value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedStyle {
    // Position
    pub position: Option<String>,      // "fixed", "relative"
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
    // Size
    pub width: Option<String>,
    pub height: Option<String>,
    // Visual
    pub background: Option<String>,
    pub color: Option<String>,
    pub opacity: Option<f32>,
    pub border_radius: Option<String>,
    // Typography
    pub font_size: Option<String>,
    pub font_weight: Option<String>,
    pub font_family: Option<String>,
    // Flexbox
    pub display: Option<String>,
    pub flex_direction: Option<String>,
    pub justify_content: Option<String>,
    pub align_items: Option<String>,
    pub gap: Option<String>,
    // Padding/margin
    pub padding: Option<String>,
    pub margin: Option<String>,
}

impl Default for ResolvedStyle {
    fn default() -> Self {
        Self {
            position: None,
            top: None,
            right: None,
            bottom: None,
            left: None,
            width: None,
            height: None,
            background: None,
            color: None,
            opacity: None,
            border_radius: None,
            font_size: None,
            font_weight: None,
            font_family: None,
            display: None,
            flex_direction: None,
            justify_content: None,
            align_items: None,
            gap: None,
            padding: None,
            margin: None,
        }
    }
}

impl OmniFile {
    /// Create an empty OmniFile.
    pub fn empty() -> Self {
        Self {
            theme_src: None,
            widgets: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omni_file_serializes_to_json() {
        let file = OmniFile {
            theme_src: Some("./themes/dark.css".to_string()),
            widgets: vec![Widget {
                id: "fps".to_string(),
                name: "FPS Counter".to_string(),
                enabled: true,
                template: HtmlNode::Element {
                    tag: "div".to_string(),
                    id: Some("fps".to_string()),
                    classes: vec![],
                    inline_style: None,
                    children: vec![HtmlNode::Text {
                        content: "{fps}".to_string(),
                    }],
                },
                style_source: "#fps { color: white; }".to_string(),
            }],
        };

        let json = serde_json::to_string(&file).unwrap();
        let deserialized: OmniFile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.widgets.len(), 1);
        assert_eq!(deserialized.widgets[0].id, "fps");
    }

    #[test]
    fn html_node_element_serializes() {
        let node = HtmlNode::Element {
            tag: "div".to_string(),
            id: None,
            classes: vec!["panel".to_string(), "active".to_string()],
            inline_style: Some("color: red;".to_string()),
            children: vec![],
        };

        let json = serde_json::to_string(&node).unwrap();
        assert!(json.contains("panel"));
        assert!(json.contains("active"));
    }
}
```

- [ ] **Step 2: Create host/src/omni/mod.rs**

```rust
pub mod types;
pub mod parser;
pub mod css;
pub mod resolver;
pub mod interpolation;
pub mod sensor_map;
pub mod default;

pub use types::*;
```

- [ ] **Step 3: Add mod declaration to main.rs**

Add `mod omni;` after `mod widget_builder;`:

```rust
mod widget_builder;
mod ws_server;
mod omni;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Compiles (the sub-modules don't exist yet — create empty placeholder files if needed).

Create empty placeholder files so the module declarations don't error:
```bash
mkdir -p host/src/omni
echo "" > host/src/omni/parser.rs
echo "" > host/src/omni/css.rs
echo "" > host/src/omni/resolver.rs
echo "" > host/src/omni/interpolation.rs
echo "" > host/src/omni/sensor_map.rs
echo "" > host/src/omni/default.rs
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p omni-host -- omni::types`
Expected: 2 tests pass.

- [ ] **Step 6: Commit**

```bash
git add host/src/omni/ host/src/main.rs
git commit -m "feat(host): add .omni file data types (OmniFile, Widget, HtmlNode)"
```

---

### Task 3: Sensor Map — Path String to Value

**Files:**
- Create: `host/src/omni/sensor_map.rs`

Maps sensor path strings (e.g., `"cpu.usage"`) to `SensorSource` enum variants and extracts current values from a `SensorSnapshot`.

- [ ] **Step 1: Create host/src/omni/sensor_map.rs**

```rust
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
        "gpu.vram" => format!("{}/{}", snapshot.gpu.vram_used_mb, snapshot.gpu.vram_total_mb),
        "gpu.vram.used" => format!("{}", snapshot.gpu.vram_used_mb),
        "gpu.vram.total" => format!("{}", snapshot.gpu.vram_total_mb),
        "gpu.power" => format!("{:.0}", snapshot.gpu.power_draw_w),
        "gpu.fan" => format!("{}", snapshot.gpu.fan_speed_percent),
        "ram.usage" => format!("{:.0}", snapshot.ram.usage_percent),
        "ram.used" => format!("{}", snapshot.ram.used_mb),
        "ram.total" => format!("{}", snapshot.ram.total_mb),
        "fps" => "N/A".to_string(), // overridden by DLL frame stats
        "frame-time" => "N/A".to_string(),
        "frame-time.avg" => "N/A".to_string(),
        "frame-time.1pct" => "N/A".to_string(),
        "frame-time.01pct" => "N/A".to_string(),
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
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host -- omni::sensor_map`
Expected: 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/omni/sensor_map.rs
git commit -m "feat(host): add sensor path mapping (string → SensorSource + value)"
```

---

### Task 4: Sensor Interpolation

**Files:**
- Create: `host/src/omni/interpolation.rs`

Handles `{sensor.path}` replacement in text content and style values.

- [ ] **Step 1: Create host/src/omni/interpolation.rs**

```rust
//! Interpolation of {sensor.path} expressions in text and style values.
//!
//! Scans a string for `{...}` patterns, looks up each path in the sensor map,
//! and replaces it with the current value.

use omni_shared::SensorSnapshot;
use super::sensor_map;

/// Replace all `{sensor.path}` expressions in the input string with current values.
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

/// Check if a string contains any `{...}` interpolation expressions.
pub fn has_interpolation(input: &str) -> bool {
    let mut in_brace = false;
    for ch in input.chars() {
        if ch == '{' {
            in_brace = true;
        } else if ch == '}' && in_brace {
            return true;
        }
    }
    false
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

    #[test]
    fn has_interpolation_works() {
        assert!(has_interpolation("CPU: {cpu.usage}%"));
        assert!(!has_interpolation("plain text"));
        assert!(!has_interpolation("{ unclosed"));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host -- omni::interpolation`
Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/omni/interpolation.rs
git commit -m "feat(host): add {sensor.path} interpolation for text and style values"
```

---

### Task 5: .omni File Parser

**Files:**
- Rewrite: `host/src/omni/parser.rs`

Parses `.omni` source text into an `OmniFile` using `quick-xml`.

- [ ] **Step 1: Create host/src/omni/parser.rs**

```rust
//! Parser for .omni file format.
//!
//! A .omni file contains:
//! - Optional <theme src="..."/> directive
//! - One or more <widget id="..." name="..." enabled="true/false"> blocks
//!   - Each widget contains <template>...</template> and <style>...</style>

use quick_xml::events::{Event, BytesStart};
use quick_xml::Reader;
use tracing::warn;

use super::types::{OmniFile, Widget, HtmlNode};

/// Parse a .omni source string into an OmniFile.
pub fn parse_omni(source: &str) -> Result<OmniFile, Vec<ParseError>> {
    let mut errors = Vec::new();
    let mut theme_src = None;
    let mut widgets = Vec::new();

    let mut reader = Reader::from_str(source);
    reader.config_mut().trim_text(true);

    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.name().as_ref() {
                    b"widget" => {
                        match parse_widget(&mut reader, e) {
                            Ok(widget) => widgets.push(widget),
                            Err(e) => errors.push(e),
                        }
                    }
                    b"theme" => {
                        theme_src = get_attr(e, "src");
                    }
                    other => {
                        let name = String::from_utf8_lossy(other).to_string();
                        errors.push(ParseError {
                            message: format!("Unknown top-level element <{}>", name),
                            line: reader.buffer_position(),
                        });
                    }
                }
            }
            Ok(Event::Empty(ref e)) => {
                if e.name().as_ref() == b"theme" {
                    theme_src = get_attr(e, "src");
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                errors.push(ParseError {
                    message: format!("XML parse error: {}", e),
                    line: reader.buffer_position(),
                });
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    if errors.is_empty() {
        Ok(OmniFile { theme_src, widgets })
    } else {
        Err(errors)
    }
}

/// Parse a <widget> element and its children (<template> and <style>).
fn parse_widget(reader: &mut Reader<&[u8]>, start: &BytesStart) -> Result<Widget, ParseError> {
    let id = get_attr(start, "id")
        .ok_or_else(|| ParseError {
            message: "Widget missing required 'id' attribute".to_string(),
            line: reader.buffer_position(),
        })?;

    let name = get_attr(start, "name")
        .ok_or_else(|| ParseError {
            message: format!("Widget '{}' missing required 'name' attribute", id),
            line: reader.buffer_position(),
        })?;

    let enabled = get_attr(start, "enabled")
        .map(|v| v != "false")
        .unwrap_or(true);

    let mut template: Option<HtmlNode> = None;
    let mut style_source = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.name().as_ref() {
                    b"template" => {
                        template = Some(parse_template_children(reader)?);
                    }
                    b"style" => {
                        style_source = read_text_content(reader, "style")?;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"widget" => break,
            Ok(Event::Eof) => {
                return Err(ParseError {
                    message: format!("Unexpected EOF inside widget '{}'", id),
                    line: reader.buffer_position(),
                });
            }
            Err(e) => {
                return Err(ParseError {
                    message: format!("XML error in widget '{}': {}", id, e),
                    line: reader.buffer_position(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    let template = template.unwrap_or(HtmlNode::Element {
        tag: "div".to_string(),
        id: None,
        classes: vec![],
        inline_style: None,
        children: vec![],
    });

    Ok(Widget {
        id,
        name,
        enabled,
        template,
        style_source,
    })
}

/// Parse the children of a <template> element into an HtmlNode tree.
/// Wraps multiple root elements in a synthetic <div>.
fn parse_template_children(reader: &mut Reader<&[u8]>) -> Result<HtmlNode, ParseError> {
    let mut children = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let node = parse_html_element(reader, e)?;
                children.push(node);
            }
            Ok(Event::Empty(ref e)) => {
                let node = parse_empty_html_element(e);
                children.push(node);
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if !text.trim().is_empty() {
                    children.push(HtmlNode::Text { content: text });
                }
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"template" => break,
            Ok(Event::Eof) => {
                return Err(ParseError {
                    message: "Unexpected EOF inside <template>".to_string(),
                    line: reader.buffer_position(),
                });
            }
            Err(e) => {
                return Err(ParseError {
                    message: format!("XML error in template: {}", e),
                    line: reader.buffer_position(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    // If there's exactly one child, use it as the root. Otherwise, wrap in a div.
    if children.len() == 1 {
        Ok(children.remove(0))
    } else {
        Ok(HtmlNode::Element {
            tag: "div".to_string(),
            id: None,
            classes: vec![],
            inline_style: None,
            children,
        })
    }
}

/// Parse an HTML element with children.
fn parse_html_element(reader: &mut Reader<&[u8]>, start: &BytesStart) -> Result<HtmlNode, ParseError> {
    let tag = String::from_utf8_lossy(start.name().as_ref()).to_string();
    let id = get_attr(start, "id");
    let classes = get_attr(start, "class")
        .map(|c| c.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let inline_style = get_attr(start, "style");

    let mut children = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let child = parse_html_element(reader, e)?;
                children.push(child);
            }
            Ok(Event::Empty(ref e)) => {
                children.push(parse_empty_html_element(e));
            }
            Ok(Event::Text(ref e)) => {
                let text = e.unescape().unwrap_or_default().to_string();
                if !text.trim().is_empty() {
                    children.push(HtmlNode::Text { content: text });
                }
            }
            Ok(Event::End(ref e)) => {
                let end_tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if end_tag == tag {
                    break;
                }
            }
            Ok(Event::Eof) => {
                return Err(ParseError {
                    message: format!("Unexpected EOF inside <{}>", tag),
                    line: reader.buffer_position(),
                });
            }
            Err(e) => {
                return Err(ParseError {
                    message: format!("XML error in <{}>: {}", tag, e),
                    line: reader.buffer_position(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(HtmlNode::Element {
        tag,
        id,
        classes,
        inline_style,
        children,
    })
}

/// Parse a self-closing HTML element (e.g., <br/>, <spacer/>).
fn parse_empty_html_element(start: &BytesStart) -> HtmlNode {
    let tag = String::from_utf8_lossy(start.name().as_ref()).to_string();
    let id = get_attr(start, "id");
    let classes = get_attr(start, "class")
        .map(|c| c.split_whitespace().map(String::from).collect())
        .unwrap_or_default();
    let inline_style = get_attr(start, "style");

    HtmlNode::Element {
        tag,
        id,
        classes,
        inline_style,
        children: vec![],
    }
}

/// Read all text content until the closing tag is found.
fn read_text_content(reader: &mut Reader<&[u8]>, tag: &str) -> Result<String, ParseError> {
    let mut content = String::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Text(ref e)) | Ok(Event::CData(ref e)) => {
                content.push_str(&e.unescape().unwrap_or_default());
            }
            Ok(Event::End(ref e)) => {
                if e.name().as_ref() == tag.as_bytes() {
                    break;
                }
            }
            Ok(Event::Eof) => {
                return Err(ParseError {
                    message: format!("Unexpected EOF inside <{}>", tag),
                    line: reader.buffer_position(),
                });
            }
            Err(e) => {
                return Err(ParseError {
                    message: format!("XML error reading <{}>: {}", tag, e),
                    line: reader.buffer_position(),
                });
            }
            _ => {}
        }
        buf.clear();
    }

    Ok(content)
}

/// Extract an attribute value from an XML element.
fn get_attr(start: &BytesStart, name: &str) -> Option<String> {
    start.attributes()
        .filter_map(|a| a.ok())
        .find(|a| a.key.as_ref() == name.as_bytes())
        .map(|a| String::from_utf8_lossy(&a.value).to_string())
}

/// A parse error with position information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(serde::Serialize, serde::Deserialize)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
}

// Remove the duplicate derive — fix:
// The struct should only have one set of derives. Use:
// #[derive(Debug, Clone)]  and implement Serialize/Deserialize via serde.
// Actually, let's just use serde from the parent module.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_omni() {
        let source = r#"
            <widget id="test" name="Test Widget" enabled="true">
                <template>
                    <div class="panel">
                        <span>Hello</span>
                    </div>
                </template>
                <style>
                    .panel { color: white; }
                </style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        assert_eq!(file.widgets.len(), 1);
        assert_eq!(file.widgets[0].id, "test");
        assert_eq!(file.widgets[0].name, "Test Widget");
        assert!(file.widgets[0].enabled);
        assert!(file.widgets[0].style_source.contains("color: white"));
    }

    #[test]
    fn parse_with_theme() {
        let source = r#"
            <theme src="./themes/dark.css" />
            <widget id="fps" name="FPS" enabled="true">
                <template><span>{fps}</span></template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        assert_eq!(file.theme_src, Some("./themes/dark.css".to_string()));
    }

    #[test]
    fn parse_multiple_widgets() {
        let source = r#"
            <widget id="cpu" name="CPU Monitor" enabled="true">
                <template><div><span>CPU</span></div></template>
                <style></style>
            </widget>
            <widget id="gpu" name="GPU Monitor" enabled="false">
                <template><div><span>GPU</span></div></template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        assert_eq!(file.widgets.len(), 2);
        assert_eq!(file.widgets[0].id, "cpu");
        assert!(file.widgets[0].enabled);
        assert_eq!(file.widgets[1].id, "gpu");
        assert!(!file.widgets[1].enabled);
    }

    #[test]
    fn parse_inline_styles_and_ids() {
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template>
                    <div id="main" class="panel dark" style="position: fixed; top: 10px;">
                        <span class="value">text</span>
                    </div>
                </template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        if let HtmlNode::Element { id, classes, inline_style, children, .. } = &file.widgets[0].template {
            assert_eq!(id.as_deref(), Some("main"));
            assert_eq!(classes, &["panel", "dark"]);
            assert!(inline_style.as_ref().unwrap().contains("position: fixed"));
            assert_eq!(children.len(), 1);
        } else {
            panic!("Expected Element");
        }
    }

    #[test]
    fn parse_sensor_interpolation_in_text() {
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template>
                    <span>CPU: {cpu.usage}%</span>
                </template>
                <style></style>
            </widget>
        "#;

        let file = parse_omni(source).unwrap();
        if let HtmlNode::Element { children, .. } = &file.widgets[0].template {
            if let HtmlNode::Text { content } = &children[0] {
                assert_eq!(content, "CPU: {cpu.usage}%");
            } else {
                panic!("Expected Text node");
            }
        }
    }

    #[test]
    fn missing_widget_id_returns_error() {
        let source = r#"
            <widget name="Test" enabled="true">
                <template><div></div></template>
                <style></style>
            </widget>
        "#;

        let result = parse_omni(source);
        assert!(result.is_err());
        assert!(result.unwrap_err()[0].message.contains("missing required 'id'"));
    }
}
```

Note: The `ParseError` struct has a duplicate derive issue in the code above. The implementer should use a single `#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]` or import serde at the top. Fix this during implementation.

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo test -p omni-host -- omni::parser`
Expected: 6 tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/omni/parser.rs
git commit -m "feat(host): add .omni file parser (quick-xml → OmniFile)"
```

---

### Task 6: CSS Resolution

**Files:**
- Rewrite: `host/src/omni/css.rs`

Parses CSS with lightningcss, resolves variables, and applies styles to HTML elements. For Phase 9a-1: class selectors and ID selectors only (no descendant selectors).

- [ ] **Step 1: Create host/src/omni/css.rs**

This module needs to:
1. Parse a CSS string (from `<style>` block) into a list of rules
2. Parse theme CSS for `:root` variables
3. Given an HTML element (tag, id, classes), find matching rules and merge properties
4. Resolve `var(--name)` references
5. Parse inline style strings
6. Return a `ResolvedStyle`

```rust
//! CSS parsing and resolution for .omni files.
//!
//! Uses lightningcss to parse CSS, then resolves selectors and variables
//! against HTML elements. Phase 9a-1 supports class and ID selectors only.

use std::collections::HashMap;
use tracing::debug;

use super::types::ResolvedStyle;

/// A parsed CSS rule: selector → properties.
#[derive(Debug, Clone)]
pub struct CssRule {
    pub selector: Selector,
    pub properties: HashMap<String, String>,
}

/// A simple CSS selector (Phase 9a-1: class or ID only).
#[derive(Debug, Clone)]
pub enum Selector {
    Class(String),          // .panel
    Id(String),             // #fps
    Element(String),        // div, span
    Root,                   // :root (for variables)
}

/// Parsed stylesheet: rules + variables.
#[derive(Debug, Clone, Default)]
pub struct ParsedStylesheet {
    pub rules: Vec<CssRule>,
    pub variables: HashMap<String, String>,
}

/// Parse a CSS string into rules and variables.
/// Uses a simple hand-parser for the subset we need, backed by the structure
/// that lightningcss validates.
pub fn parse_css(source: &str) -> ParsedStylesheet {
    let mut stylesheet = ParsedStylesheet::default();

    // Simple CSS parser: split by rule blocks
    let mut remaining = source.trim();

    while !remaining.is_empty() {
        remaining = remaining.trim();
        if remaining.is_empty() {
            break;
        }

        // Find the selector (everything before '{')
        let brace_open = match remaining.find('{') {
            Some(pos) => pos,
            None => break,
        };

        let selector_str = remaining[..brace_open].trim();

        // Find matching closing brace
        let brace_close = match find_matching_brace(remaining, brace_open) {
            Some(pos) => pos,
            None => break,
        };

        let body = &remaining[brace_open + 1..brace_close];
        remaining = &remaining[brace_close + 1..];

        // Parse selector
        let selector = parse_selector(selector_str);

        // Parse properties
        let properties = parse_properties(body);

        // Extract :root variables
        if matches!(&selector, Selector::Root) {
            for (key, value) in &properties {
                if key.starts_with("--") {
                    stylesheet.variables.insert(key.clone(), value.clone());
                }
            }
        }

        stylesheet.rules.push(CssRule { selector, properties });
    }

    stylesheet
}

/// Resolve styles for an HTML element by matching CSS rules.
/// Priority: element selector < class selector < ID selector < inline style.
pub fn resolve_styles(
    tag: &str,
    id: Option<&str>,
    classes: &[String],
    inline_style: Option<&str>,
    stylesheet: &ParsedStylesheet,
    theme_vars: &HashMap<String, String>,
) -> ResolvedStyle {
    let mut merged: HashMap<String, String> = HashMap::new();

    // Apply matching rules in order (later rules override earlier)
    for rule in &stylesheet.rules {
        let matches = match &rule.selector {
            Selector::Element(el) => el == tag,
            Selector::Class(cls) => classes.iter().any(|c| c == cls),
            Selector::Id(rule_id) => id.map_or(false, |eid| eid == rule_id),
            Selector::Root => false, // :root doesn't apply to elements
        };

        if matches {
            for (key, value) in &rule.properties {
                merged.insert(key.clone(), value.clone());
            }
        }
    }

    // Apply inline styles (highest priority)
    if let Some(inline) = inline_style {
        let inline_props = parse_properties(inline);
        for (key, value) in inline_props {
            merged.insert(key, value);
        }
    }

    // Resolve var() references
    let all_vars: HashMap<String, String> = theme_vars.iter()
        .chain(stylesheet.variables.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    for value in merged.values_mut() {
        *value = resolve_variables(value, &all_vars);
    }

    // Map to ResolvedStyle
    props_to_resolved_style(&merged)
}

/// Resolve var(--name) references in a CSS value.
fn resolve_variables(value: &str, variables: &HashMap<String, String>) -> String {
    let mut result = value.to_string();
    // Iteratively resolve var() references (handles nested vars)
    for _ in 0..10 {
        if !result.contains("var(") {
            break;
        }
        let mut new_result = String::new();
        let mut chars = result.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == 'v' && chars.peek() == Some(&'a') {
                // Check for "var("
                let mut peek: String = String::from(ch);
                let mut matched = false;
                let saved = chars.clone();
                for expected in ['a', 'r', '('] {
                    if let Some(&next) = chars.peek() {
                        if next == expected {
                            peek.push(chars.next().unwrap());
                        }
                    }
                }
                if peek == "var(" {
                    // Read until closing paren
                    let mut var_name = String::new();
                    let mut depth = 1;
                    for inner in chars.by_ref() {
                        if inner == '(' {
                            depth += 1;
                        } else if inner == ')' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        var_name.push(inner);
                    }
                    let var_name = var_name.trim();
                    // Check for fallback: var(--name, fallback)
                    let (name, fallback) = if let Some(comma_pos) = var_name.find(',') {
                        (&var_name[..comma_pos], Some(var_name[comma_pos + 1..].trim()))
                    } else {
                        (var_name, None)
                    };
                    let name = name.trim();
                    if let Some(resolved) = variables.get(name) {
                        new_result.push_str(resolved);
                    } else if let Some(fb) = fallback {
                        new_result.push_str(fb);
                    } else {
                        new_result.push_str(value); // unresolved
                    }
                    matched = true;
                }
                if !matched {
                    new_result.push_str(&peek);
                }
            } else {
                new_result.push(ch);
            }
        }
        result = new_result;
    }
    result
}

fn parse_selector(s: &str) -> Selector {
    let s = s.trim();
    if s == ":root" {
        Selector::Root
    } else if s.starts_with('#') {
        Selector::Id(s[1..].to_string())
    } else if s.starts_with('.') {
        Selector::Class(s[1..].to_string())
    } else {
        Selector::Element(s.to_string())
    }
}

fn parse_properties(body: &str) -> HashMap<String, String> {
    let mut props = HashMap::new();
    for decl in body.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some(colon) = decl.find(':') {
            let key = decl[..colon].trim().to_string();
            let value = decl[colon + 1..].trim().to_string();
            if !key.is_empty() && !value.is_empty() {
                props.insert(key, value);
            }
        }
    }
    props
}

fn find_matching_brace(s: &str, open_pos: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in s[open_pos..].char_indices() {
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                return Some(open_pos + i);
            }
        }
    }
    None
}

fn props_to_resolved_style(props: &HashMap<String, String>) -> ResolvedStyle {
    ResolvedStyle {
        position: props.get("position").cloned(),
        top: props.get("top").cloned(),
        right: props.get("right").cloned(),
        bottom: props.get("bottom").cloned(),
        left: props.get("left").cloned(),
        width: props.get("width").cloned(),
        height: props.get("height").cloned(),
        background: props.get("background").cloned(),
        color: props.get("color").cloned(),
        opacity: props.get("opacity").and_then(|v| v.parse().ok()),
        border_radius: props.get("border-radius").cloned(),
        font_size: props.get("font-size").cloned(),
        font_weight: props.get("font-weight").cloned(),
        font_family: props.get("font-family").cloned(),
        display: props.get("display").cloned(),
        flex_direction: props.get("flex-direction").cloned(),
        justify_content: props.get("justify-content").cloned(),
        align_items: props.get("align-items").cloned(),
        gap: props.get("gap").cloned(),
        padding: props.get("padding").cloned(),
        margin: props.get("margin").cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_css() {
        let css = ".panel { color: white; font-size: 14px; }";
        let sheet = parse_css(css);
        assert_eq!(sheet.rules.len(), 1);
        assert_eq!(sheet.rules[0].properties.get("color").unwrap(), "white");
        assert_eq!(sheet.rules[0].properties.get("font-size").unwrap(), "14px");
    }

    #[test]
    fn parse_root_variables() {
        let css = ":root { --bg: rgba(20,20,20,0.7); --text: #ffffff; }";
        let sheet = parse_css(css);
        assert_eq!(sheet.variables.get("--bg").unwrap(), "rgba(20,20,20,0.7)");
        assert_eq!(sheet.variables.get("--text").unwrap(), "#ffffff");
    }

    #[test]
    fn resolve_class_selector() {
        let css = ".panel { color: white; } .value { font-size: 14px; }";
        let sheet = parse_css(css);
        let style = resolve_styles(
            "div", None, &["panel".to_string()], None,
            &sheet, &HashMap::new(),
        );
        assert_eq!(style.color.as_deref(), Some("white"));
        assert!(style.font_size.is_none()); // .value doesn't match
    }

    #[test]
    fn resolve_id_selector() {
        let css = "#fps { color: green; }";
        let sheet = parse_css(css);
        let style = resolve_styles(
            "span", Some("fps"), &[], None,
            &sheet, &HashMap::new(),
        );
        assert_eq!(style.color.as_deref(), Some("green"));
    }

    #[test]
    fn inline_style_overrides() {
        let css = ".panel { color: white; }";
        let sheet = parse_css(css);
        let style = resolve_styles(
            "div", None, &["panel".to_string()],
            Some("color: red; font-size: 20px;"),
            &sheet, &HashMap::new(),
        );
        assert_eq!(style.color.as_deref(), Some("red"));
        assert_eq!(style.font_size.as_deref(), Some("20px"));
    }

    #[test]
    fn variable_resolution() {
        let css = ":root { --bg: red; } .panel { background: var(--bg); }";
        let sheet = parse_css(css);
        let style = resolve_styles(
            "div", None, &["panel".to_string()], None,
            &sheet, &HashMap::new(),
        );
        assert_eq!(style.background.as_deref(), Some("red"));
    }

    #[test]
    fn theme_variables_apply() {
        let css = ".panel { color: var(--text); }";
        let sheet = parse_css(css);
        let mut theme_vars = HashMap::new();
        theme_vars.insert("--text".to_string(), "#ffffff".to_string());

        let style = resolve_styles(
            "div", None, &["panel".to_string()], None,
            &sheet, &theme_vars,
        );
        assert_eq!(style.color.as_deref(), Some("#ffffff"));
    }
}
```

Note: This task uses a hand-written CSS parser for Phase 9a-1 simplicity. `lightningcss` will be integrated in Phase 9a-2 when we need the full CSS grammar for animations, transforms, and advanced selectors. The dependency is added now so it's available, but the simple parser handles our current subset well and gives us better control over error messages.

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host -- omni::css`
Expected: 7 tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/omni/css.rs
git commit -m "feat(host): add CSS parsing and resolution with variable support"
```

---

### Task 7: OmniResolver — Full Pipeline

**Files:**
- Rewrite: `host/src/omni/resolver.rs`

Takes an `OmniFile` + `SensorSnapshot` and outputs `Vec<ComputedWidget>`.

- [ ] **Step 1: Create host/src/omni/resolver.rs**

```rust
//! Resolves an OmniFile into ComputedWidgets for rendering.
//!
//! Pipeline: OmniFile → for each enabled widget → resolve CSS → interpolate
//! sensor values → emit ComputedWidget for each HTML element.

use std::collections::HashMap;

use omni_shared::{ComputedWidget, SensorSnapshot, WidgetType, SensorSource, write_fixed_str};

use super::types::{OmniFile, Widget, HtmlNode, ResolvedStyle};
use super::css::{self, ParsedStylesheet};
use super::interpolation;
use super::sensor_map;

/// Resolves an OmniFile into a flat list of ComputedWidgets.
pub struct OmniResolver {
    /// Theme CSS variables (loaded from theme file).
    theme_vars: HashMap<String, String>,
}

impl OmniResolver {
    pub fn new() -> Self {
        Self {
            theme_vars: HashMap::new(),
        }
    }

    /// Load theme variables from a CSS source string.
    pub fn load_theme(&mut self, theme_css: &str) {
        let sheet = css::parse_css(theme_css);
        self.theme_vars = sheet.variables;
    }

    /// Resolve the OmniFile into ComputedWidgets using current sensor data.
    pub fn resolve(&self, file: &OmniFile, snapshot: &SensorSnapshot) -> Vec<ComputedWidget> {
        let mut widgets = Vec::new();

        for widget_def in &file.widgets {
            if !widget_def.enabled {
                continue;
            }

            let stylesheet = css::parse_css(&widget_def.style_source);
            self.resolve_node(
                &widget_def.template,
                &stylesheet,
                snapshot,
                &mut widgets,
                0.0, 0.0, // parent offset
            );
        }

        widgets
    }

    fn resolve_node(
        &self,
        node: &HtmlNode,
        stylesheet: &ParsedStylesheet,
        snapshot: &SensorSnapshot,
        out: &mut Vec<ComputedWidget>,
        parent_x: f32,
        parent_y: f32,
    ) {
        match node {
            HtmlNode::Element { tag, id, classes, inline_style, children } => {
                // Resolve CSS for this element
                let interpolated_inline = inline_style.as_ref()
                    .map(|s| interpolation::interpolate(s, snapshot));

                let style = css::resolve_styles(
                    tag,
                    id.as_deref(),
                    classes,
                    interpolated_inline.as_deref(),
                    stylesheet,
                    &self.theme_vars,
                );

                // Compute position
                let x = parse_px(style.left.as_deref()).unwrap_or(parent_x);
                let y = parse_px(style.top.as_deref()).unwrap_or(parent_y);
                let width = parse_px(style.width.as_deref()).unwrap_or(200.0);
                let height = parse_px(style.height.as_deref()).unwrap_or(0.0);

                // Check if this is a container (has children) or a leaf
                let has_text_children = children.iter().any(|c| matches!(c, HtmlNode::Text { .. }));

                if has_text_children {
                    // Collect text content
                    let text: String = children.iter()
                        .filter_map(|c| match c {
                            HtmlNode::Text { content } => Some(interpolation::interpolate(content, snapshot)),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("");

                    // Determine sensor source from text content
                    let source = detect_sensor_source(&text, children);

                    let mut cw = style_to_computed_widget(&style, x, y, width);
                    cw.widget_type = WidgetType::SensorValue;
                    cw.source = source;

                    // Auto-calculate height if not specified
                    if height > 0.0 {
                        cw.height = height;
                    } else {
                        cw.height = cw.font_size + 8.0; // font size + padding
                    }

                    write_fixed_str(&mut cw.format_pattern, &text);
                    out.push(cw);
                } else if !children.is_empty() {
                    // Container element — emit background if styled, then process children
                    let bg = parse_color(style.background.as_deref());
                    if bg[3] > 0 || style.border_radius.is_some() {
                        // Emit a background widget for the container
                        let mut cw = style_to_computed_widget(&style, x, y, width);
                        cw.widget_type = WidgetType::Group;
                        cw.source = SensorSource::None;

                        // Calculate container height from children
                        let child_height = estimate_children_height(children, &style);
                        cw.height = if height > 0.0 { height } else { child_height };

                        out.push(cw);
                    }

                    // Layout children
                    let padding = parse_px(style.padding.as_deref()).unwrap_or(0.0);
                    let gap = parse_px(style.gap.as_deref()).unwrap_or(0.0);
                    let is_row = style.flex_direction.as_deref() == Some("row");

                    let mut child_x = x + padding;
                    let mut child_y = y + padding;

                    for child in children {
                        self.resolve_node(child, stylesheet, snapshot, out, child_x, child_y);

                        if is_row {
                            child_x += width + gap; // simplified: each child gets same width
                        } else {
                            // Estimate child height for vertical stacking
                            let ch = estimate_node_height(child, &style);
                            child_y += ch + gap;
                        }
                    }
                } else {
                    // Empty element (spacer, decoration)
                    if height > 0.0 || parse_color(style.background.as_deref())[3] > 0 {
                        let mut cw = style_to_computed_widget(&style, x, y, width);
                        cw.widget_type = WidgetType::Spacer;
                        cw.height = height;
                        out.push(cw);
                    }
                }
            }
            HtmlNode::Text { .. } => {
                // Text nodes are handled by their parent Element
            }
        }
    }
}

/// Convert a ResolvedStyle into a ComputedWidget with position and visual properties.
fn style_to_computed_widget(style: &ResolvedStyle, x: f32, y: f32, default_width: f32) -> ComputedWidget {
    let mut cw = ComputedWidget::default();
    cw.x = x;
    cw.y = y;
    cw.width = parse_px(style.width.as_deref()).unwrap_or(default_width);
    cw.opacity = style.opacity.unwrap_or(1.0);
    cw.font_size = parse_px(style.font_size.as_deref()).unwrap_or(14.0);
    cw.font_weight = style.font_weight.as_deref()
        .and_then(|w| match w {
            "bold" => Some(700),
            "normal" => Some(400),
            _ => w.parse().ok(),
        })
        .unwrap_or(400);
    cw.color_rgba = parse_color(style.color.as_deref());
    cw.bg_color_rgba = parse_color(style.background.as_deref());

    if let Some(br) = &style.border_radius {
        let r = parse_px(Some(br)).unwrap_or(0.0);
        cw.border_radius = [r; 4];
    }

    cw
}

/// Parse a CSS color value into [r, g, b, a].
fn parse_color(value: Option<&str>) -> [u8; 4] {
    let value = match value {
        Some(v) => v.trim(),
        None => return [0, 0, 0, 0],
    };

    // Hex colors
    if value.starts_with('#') {
        return parse_hex_color(value);
    }

    // rgba(r, g, b, a)
    if value.starts_with("rgba(") {
        if let Some(inner) = value.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() == 4 {
                let r = parts[0].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let g = parts[1].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let b = parts[2].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let a = (parts[3].trim().parse::<f32>().unwrap_or(1.0) * 255.0) as u8;
                return [r, g, b, a];
            }
        }
    }

    // rgb(r, g, b)
    if value.starts_with("rgb(") {
        if let Some(inner) = value.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() == 3 {
                let r = parts[0].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let g = parts[1].trim().parse::<f32>().unwrap_or(0.0) as u8;
                let b = parts[2].trim().parse::<f32>().unwrap_or(0.0) as u8;
                return [r, g, b, 255];
            }
        }
    }

    // Named colors (common ones)
    match value {
        "white" => [255, 255, 255, 255],
        "black" => [0, 0, 0, 255],
        "red" => [255, 0, 0, 255],
        "green" => [0, 128, 0, 255],
        "blue" => [0, 0, 255, 255],
        "transparent" => [0, 0, 0, 0],
        _ => [0, 0, 0, 0],
    }
}

fn parse_hex_color(hex: &str) -> [u8; 4] {
    let hex = hex.trim_start_matches('#');
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).unwrap_or(0);
            [r, g, b, 255]
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            [r, g, b, 255]
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
            let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
            let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
            let a = u8::from_str_radix(&hex[6..8], 16).unwrap_or(255);
            [r, g, b, a]
        }
        _ => [0, 0, 0, 0],
    }
}

/// Parse a CSS pixel value (e.g., "10px", "14") into f32.
fn parse_px(value: Option<&str>) -> Option<f32> {
    let v = value?.trim();
    let v = v.strip_suffix("px").unwrap_or(v);
    v.parse().ok()
}

/// Detect the primary sensor source from text content.
fn detect_sensor_source(text: &str, children: &[HtmlNode]) -> SensorSource {
    // Look for {sensor.path} in the original template text
    for child in children {
        if let HtmlNode::Text { content } = child {
            if let Some(start) = content.find('{') {
                if let Some(end) = content[start..].find('}') {
                    let path = content[start + 1..start + end].trim();
                    if let Some(source) = sensor_map::parse_sensor_path(path) {
                        return source;
                    }
                }
            }
        }
    }
    SensorSource::None
}

/// Estimate the height of child nodes for container sizing.
fn estimate_children_height(children: &[HtmlNode], parent_style: &ResolvedStyle) -> f32 {
    let gap = parse_px(parent_style.gap.as_deref()).unwrap_or(0.0);
    let padding = parse_px(parent_style.padding.as_deref()).unwrap_or(0.0);
    let font_size = parse_px(parent_style.font_size.as_deref()).unwrap_or(14.0);

    let mut total = padding * 2.0;
    let count = children.len();
    for (i, child) in children.iter().enumerate() {
        total += estimate_node_height(child, parent_style);
        if i < count - 1 {
            total += gap;
        }
    }
    total
}

fn estimate_node_height(node: &HtmlNode, parent_style: &ResolvedStyle) -> f32 {
    match node {
        HtmlNode::Text { .. } => {
            parse_px(parent_style.font_size.as_deref()).unwrap_or(14.0) + 8.0
        }
        HtmlNode::Element { children, inline_style, .. } => {
            // Check for explicit height in inline style
            if let Some(style) = inline_style {
                if let Some(h) = style.split(';')
                    .find(|s| s.trim().starts_with("height"))
                    .and_then(|s| s.split(':').nth(1))
                    .and_then(|v| parse_px(Some(v)))
                {
                    return h;
                }
            }
            if children.is_empty() {
                0.0
            } else {
                let font_size = parse_px(parent_style.font_size.as_deref()).unwrap_or(14.0);
                font_size + 8.0 // default text element height
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::parser;

    #[test]
    fn resolve_simple_widget() {
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template>
                    <div class="panel" style="position: fixed; top: 10px; left: 20px;">
                        <span class="val">CPU: {cpu.usage}%</span>
                    </div>
                </template>
                <style>
                    .panel { background: rgba(20,20,20,180); border-radius: 8px; padding: 8px; }
                    .val { color: white; font-size: 14px; }
                </style>
            </widget>
        "#;

        let file = parser::parse_omni(source).unwrap();
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 42.0;

        let resolver = OmniResolver::new();
        let widgets = resolver.resolve(&file, &snapshot);

        assert!(!widgets.is_empty(), "Should produce at least one widget");

        // Check that the text was interpolated
        let text_widget = widgets.iter()
            .find(|w| w.widget_type == WidgetType::SensorValue)
            .expect("Should have a SensorValue widget");
        let text = omni_shared::read_fixed_str(&text_widget.format_pattern);
        assert!(text.contains("42"), "Should contain interpolated CPU value, got: {text}");
    }

    #[test]
    fn disabled_widget_produces_nothing() {
        let source = r#"
            <widget id="test" name="Test" enabled="false">
                <template><span>Hidden</span></template>
                <style></style>
            </widget>
        "#;

        let file = parser::parse_omni(source).unwrap();
        let resolver = OmniResolver::new();
        let widgets = resolver.resolve(&file, &SensorSnapshot::default());
        assert!(widgets.is_empty());
    }

    #[test]
    fn theme_variables_resolve() {
        let source = r#"
            <widget id="test" name="Test" enabled="true">
                <template><span class="val">test</span></template>
                <style>.val { color: var(--text); }</style>
            </widget>
        "#;

        let file = parser::parse_omni(source).unwrap();
        let mut resolver = OmniResolver::new();
        resolver.load_theme(":root { --text: #ff0000; }");

        let widgets = resolver.resolve(&file, &SensorSnapshot::default());
        let w = &widgets[0];
        assert_eq!(w.color_rgba, [255, 0, 0, 255], "Should resolve theme variable to red");
    }

    #[test]
    fn parse_color_formats() {
        assert_eq!(parse_color(Some("#ff0000")), [255, 0, 0, 255]);
        assert_eq!(parse_color(Some("#f00")), [255, 0, 0, 255]);
        assert_eq!(parse_color(Some("rgba(255, 0, 0, 0.5)")), [255, 0, 0, 127]);
        assert_eq!(parse_color(Some("white")), [255, 255, 255, 255]);
        assert_eq!(parse_color(Some("transparent")), [0, 0, 0, 0]);
        assert_eq!(parse_color(None), [0, 0, 0, 0]);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host -- omni::resolver`
Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/omni/resolver.rs
git commit -m "feat(host): add OmniResolver — .omni file → ComputedWidget pipeline"
```

---

### Task 8: Default .omni File

**Files:**
- Rewrite: `host/src/omni/default.rs`

Built-in default `.omni` content that replicates the current hardcoded sensor dashboard.

- [ ] **Step 1: Create host/src/omni/default.rs**

```rust
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
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host -- omni::default`
Expected: 1 test passes.

- [ ] **Step 3: Commit**

```bash
git add host/src/omni/default.rs
git commit -m "feat(host): add default .omni file content (system stats dashboard)"
```

---

### Task 9: Wire into Main Loop + Delete WidgetBuilder

**Files:**
- Modify: `host/src/main.rs`
- Delete: `host/src/widget_builder.rs`

Replace the `WidgetBuilder` with `OmniResolver` in the main loop.

- [ ] **Step 1: Update main.rs**

In the `run_host` function, replace:
```rust
    let widget_builder = widget_builder::WidgetBuilder::new();
```

With:
```rust
    // Load .omni file (user's overlay or built-in default)
    let omni_path = config::config_dir().join("overlay.omni");
    let omni_source = if omni_path.exists() {
        match std::fs::read_to_string(&omni_path) {
            Ok(s) => {
                info!(path = %omni_path.display(), "Loaded user overlay file");
                s
            }
            Err(e) => {
                warn!(path = %omni_path.display(), error = %e, "Failed to read overlay file, using default");
                omni::default::DEFAULT_OMNI.to_string()
            }
        }
    } else {
        info!("No overlay file found, using built-in default");
        omni::default::DEFAULT_OMNI.to_string()
    };

    let omni_file = match omni::parser::parse_omni(&omni_source) {
        Ok(f) => {
            info!(widgets = f.widgets.len(), "Overlay file parsed");
            f
        }
        Err(errors) => {
            for e in &errors {
                error!(message = %e.message, "Overlay parse error");
            }
            info!("Falling back to built-in default");
            omni::parser::parse_omni(omni::default::DEFAULT_OMNI).unwrap()
        }
    };

    let mut omni_resolver = omni::resolver::OmniResolver::new();

    // Load theme if specified
    if let Some(theme_src) = &omni_file.theme_src {
        let theme_path = omni_path.parent().unwrap_or(Path::new(".")).join(theme_src);
        if let Ok(theme_css) = std::fs::read_to_string(&theme_path) {
            omni_resolver.load_theme(&theme_css);
            info!(path = %theme_path.display(), "Theme loaded");
        } else {
            warn!(path = %theme_path.display(), "Theme file not found");
        }
    }
```

And in the main loop, replace:
```rust
        let widgets = widget_builder.build(&latest_snapshot);
```

With:
```rust
        let widgets = omni_resolver.resolve(&omni_file, &latest_snapshot);
```

Add necessary imports at the top:
```rust
use tracing::{info, error, warn};
```

Remove `mod widget_builder;` and add a `use` for the warn macro.

Also add a `config_dir()` function to config.rs (or use the parent of `config_path()`). The simplest approach: derive it from `config_path()`:

In main.rs, compute the omni path as:
```rust
    let omni_path = config::config_path().parent()
        .map(|p| p.join("overlay.omni"))
        .unwrap_or_else(|| PathBuf::from("overlay.omni"));
```

- [ ] **Step 2: Delete widget_builder.rs**

```bash
rm host/src/widget_builder.rs
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test`
Expected: All tests pass. Widget builder tests are gone, omni tests replace them.

- [ ] **Step 4: Commit**

```bash
git add host/src/main.rs
git add -u host/src/widget_builder.rs
git commit -m "feat(host): replace WidgetBuilder with OmniResolver, load .omni files on startup"
```

---

### Task 10: WebSocket widget.parse and widget.update Endpoints

**Files:**
- Modify: `host/src/ws_server.rs`

Add the `widget.parse` and `widget.update` message handlers. The shared state gains an `OmniFile` that the main loop reads.

- [ ] **Step 1: Add OmniFile to WsSharedState**

In `ws_server.rs`, update `WsSharedState`:

```rust
pub struct WsSharedState {
    pub latest_snapshot: Mutex<SensorSnapshot>,
    pub active_omni_file: Mutex<Option<crate::omni::types::OmniFile>>,
    pub running: AtomicBool,
}

impl WsSharedState {
    pub fn new() -> Self {
        Self {
            latest_snapshot: Mutex::new(SensorSnapshot::default()),
            active_omni_file: Mutex::new(None),
            running: AtomicBool::new(true),
        }
    }
}
```

- [ ] **Step 2: Add widget message handlers**

In the `handle_message` function, add cases for `widget.parse` and `widget.update`:

```rust
        "widget.parse" => {
            let source = msg.get("source").and_then(|v| v.as_str()).unwrap_or("");
            match crate::omni::parser::parse_omni(source) {
                Ok(file) => {
                    let json = serde_json::to_value(&file).unwrap_or(json!(null));
                    Some(json!({
                        "type": "widget.parsed",
                        "file": json,
                        "errors": [],
                    }).to_string())
                }
                Err(errors) => {
                    let error_list: Vec<Value> = errors.iter().map(|e| json!({
                        "message": e.message,
                        "position": e.line,
                    })).collect();
                    Some(json!({
                        "type": "widget.parsed",
                        "file": null,
                        "errors": error_list,
                    }).to_string())
                }
            }
        }
        "widget.update" => {
            if let Some(file_json) = msg.get("file") {
                match serde_json::from_value::<crate::omni::types::OmniFile>(file_json.clone()) {
                    Ok(file) => {
                        if let Ok(mut active) = state.active_omni_file.lock() {
                            *active = Some(file);
                        }
                        info!("Widget tree updated via WebSocket");
                        Some(json!({"type": "widget.updated"}).to_string())
                    }
                    Err(e) => {
                        Some(json!({
                            "type": "error",
                            "message": format!("Invalid OmniFile: {}", e),
                        }).to_string())
                    }
                }
            } else {
                Some(json!({
                    "type": "error",
                    "message": "widget.update requires a 'file' field",
                }).to_string())
            }
        }
```

- [ ] **Step 3: Update main loop to check for WebSocket widget updates**

In `run_host` in main.rs, after the sensor drain and before resolving, check for WebSocket updates:

```rust
        // Check for widget updates from WebSocket (Electron app)
        if let Ok(mut active) = ws_state.active_omni_file.lock() {
            if let Some(new_file) = active.take() {
                omni_file = new_file;
                info!("Applied widget update from WebSocket");
            }
        }
```

This requires `omni_file` to be `let mut omni_file = ...` in the setup.

- [ ] **Step 4: Update tests**

Add tests for the new handlers in ws_server.rs and update existing tests for the new WsSharedState field.

- [ ] **Step 5: Verify it compiles and tests pass**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add host/src/ws_server.rs host/src/main.rs
git commit -m "feat(host): add widget.parse and widget.update WebSocket endpoints"
```

---

### Task 11: Integration Test — .omni File in Game

This is a manual integration test.

- [ ] **Step 1: Build everything**

```bash
cargo build -p omni-host && cargo build -p omni-overlay-dll
```

- [ ] **Step 2: Start with built-in default**

```bash
cargo run -p omni-host -- --service
```

Launch a game — should see the same sensor dashboard as before (replicated from `DEFAULT_OMNI`).

- [ ] **Step 3: Create a custom .omni file**

Create `%APPDATA%\Omni\overlay.omni` with custom content:

```xml
<widget id="minimal-fps" name="Minimal FPS" enabled="true">
  <template>
    <div style="position: fixed; top: 10px; right: 10px;">
      <span style="color: #44ff88; font-size: 28px; font-weight: bold;">{fps}</span>
    </div>
  </template>
  <style></style>
</widget>
```

Restart the host. The overlay should now show only a large green FPS counter in the top-right.

- [ ] **Step 4: Test WebSocket widget.parse**

From a browser console:
```javascript
const ws = new WebSocket('ws://localhost:9473');
ws.onmessage = (e) => console.log(JSON.parse(e.data));
ws.onopen = () => ws.send(JSON.stringify({
  type: 'widget.parse',
  source: '<widget id="test" name="Test" enabled="true"><template><span>{cpu.usage}%</span></template><style></style></widget>'
}));
```

Should receive a `widget.parsed` response with the OmniFile JSON.

- [ ] **Step 5: Test WebSocket widget.update**

Send a widget.update to change the overlay in real-time (while the game is running).

- [ ] **Step 6: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 9a-1 integration test"
```

---

## Phase 9a-1 Complete — Summary

At this point you have:

1. **`.omni` file format** — `<widget>` blocks with `<template>` (HTML) + `<style>` (CSS)
2. **Parser** — `quick-xml` parses `.omni` source into JSON-serializable `OmniFile`
3. **CSS resolution** — theme variables → scoped styles → inline styles, with `var()` support
4. **Sensor interpolation** — `{sensor.path}` in text content and style values
5. **OmniResolver** — `(OmniFile, SensorSnapshot) → Vec<ComputedWidget>`
6. **WebSocket endpoints** — `widget.parse` and `widget.update` for Electron integration
7. **Default .omni** — built-in dashboard replicating the Phase 7 sensor layout
8. **User customization** — create `%APPDATA%\Omni\overlay.omni` to customize

**Next:** Phase 9a-2 adds CSS cascade, themes, and descendant selectors.
