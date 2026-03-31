# Phase 9a-3: Advanced Visuals + Taffy Flexbox

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **IMPORTANT: Run superpowers:code-reviewer after EVERY subagent task. No exceptions.**

**Goal:** Replace manual position computation with the `taffy` flexbox engine (with DirectWrite text measurement for accurate content sizing), and add visual features (linear gradients, box shadows, per-corner border radius) to the D2D renderer.

**Architecture:** Two independent subsystems. (1) Layout: `taffy` computes all positions/sizes from resolved CSS flex properties. `IDWriteFactory` in the host measures text for accurate content sizing. The manual `positions`/`parent_widths`/`estimate_*` code is removed. (2) Visuals: the CSS resolver parses gradient/shadow/border-radius values, maps them to existing `ComputedWidget` fields (`GradientDef`, `ShadowDef`, `border_radius`), and the D2D renderer draws them with `ID2D1LinearGradientBrush`, blur effects, and `ID2D1PathGeometry`.

**Tech Stack:** Rust, `taffy` (flexbox engine), `windows` crate DirectWrite (host-side text measurement), D2D1 (renderer gradient/shadow/path).

**Testing notes:** Taffy layout is unit-testable with known CSS inputs. Visual feature parsing is unit-testable. D2D rendering tested manually in-game.

**Depends on:** Phase 9a-2b complete (lightningcss CSS, flat tree, resolver).

---

## File Map

```
host/
  Cargo.toml                         # Add taffy, Win32_Graphics_DirectWrite feature
  src/
    omni/
      types.rs                       # Add new ResolvedStyle fields (flex-grow, etc., box-shadow, gradient)
      css.rs                         # Parse gradient, box-shadow, border-radius shorthand
      resolver.rs                    # MAJOR REWRITE: taffy integration + DirectWrite text measurement
      layout.rs                      # NEW: taffy tree builder — maps ResolvedStyle → taffy nodes

overlay-dll/
  src/
    renderer.rs                      # Add gradient brush, shadow rendering, per-corner path geometry
```

---

### Task 1: Add Dependencies

**Files:**
- Modify: `host/Cargo.toml`

- [ ] **Step 1: Add taffy and DirectWrite feature**

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
lightningcss = "1.0.0-alpha.71"
taffy = "0.7"

[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_System_Threading",
    "Win32_System_Memory",
    "Win32_System_Diagnostics_Debug",
    "Win32_System_Diagnostics_ToolHelp",
    "Win32_System_LibraryLoader",
    "Win32_Security",
    "Win32_UI_WindowsAndMessaging",
    "Win32_System_IO",
    "Win32_Graphics_DirectWrite",
]
```

Note: Check crates.io for the latest `taffy` version. If `0.7` doesn't exist, use the latest (e.g., `0.6` or `0.5`).

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`

- [ ] **Step 3: Commit**

```bash
git add host/Cargo.toml Cargo.lock
git commit -m "feat(host): add taffy flexbox engine and DirectWrite for text measurement"
```

---

### Task 2: Expand ResolvedStyle for Flex + Visual Properties

**Files:**
- Modify: `host/src/omni/types.rs`
- Modify: `host/src/omni/css.rs` (update `props_to_resolved_style`)

- [ ] **Step 1: Add new fields to ResolvedStyle**

Add these fields to `ResolvedStyle` in `types.rs`:

```rust
pub struct ResolvedStyle {
    // Position
    pub position: Option<String>,
    pub top: Option<String>,
    pub right: Option<String>,
    pub bottom: Option<String>,
    pub left: Option<String>,
    // Size
    pub width: Option<String>,
    pub height: Option<String>,
    pub min_width: Option<String>,
    pub max_width: Option<String>,
    pub min_height: Option<String>,
    pub max_height: Option<String>,
    // Visual
    pub background: Option<String>,
    pub background_color: Option<String>,
    pub color: Option<String>,
    pub opacity: Option<f32>,
    pub border_radius: Option<String>,
    pub box_shadow: Option<String>,
    // Typography
    pub font_size: Option<String>,
    pub font_weight: Option<String>,
    pub font_family: Option<String>,
    // Flexbox
    pub display: Option<String>,
    pub flex_direction: Option<String>,
    pub justify_content: Option<String>,
    pub align_items: Option<String>,
    pub align_self: Option<String>,
    pub flex_grow: Option<String>,
    pub flex_shrink: Option<String>,
    pub flex_wrap: Option<String>,
    pub gap: Option<String>,
    // Padding/margin
    pub padding: Option<String>,
    pub margin: Option<String>,
}
```

Update the `Default` impl and `props_to_resolved_style` in `css.rs` to map these new CSS properties.

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo test -p omni-host -- omni`

- [ ] **Step 3: Commit**

```bash
git add host/src/omni/types.rs host/src/omni/css.rs
git commit -m "feat(host): expand ResolvedStyle with flex-grow, box-shadow, min/max dimensions"
```

---

### Task 3: Taffy Layout Module

**Files:**
- Create: `host/src/omni/layout.rs`
- Modify: `host/src/omni/mod.rs` (add `pub mod layout;`)

This module builds a taffy tree from flat nodes + resolved styles, computes layout, and returns positions/sizes.

- [ ] **Step 1: Create host/src/omni/layout.rs**

The module needs to:

1. Accept `&[FlatNode]`, `&[ResolvedStyle]`, and text measurements
2. Build a `taffy::TaffyTree` where each FlatNode becomes a taffy node
3. Map CSS flex properties to taffy `Style`:
   - `display: flex` → `Display::Flex`
   - `flex_direction` → `FlexDirection::Column` / `Row`
   - `justify_content` → `JustifyContent::FlexStart` / `Center` / `SpaceBetween` / etc.
   - `align_items` → `AlignItems::FlexStart` / `Center` / `Stretch` / etc.
   - `flex_grow` / `flex_shrink` → `f32` values
   - `flex_wrap` → `FlexWrap::Wrap` / `NoWrap`
   - `gap` → `Size { width, height }` in points
   - `padding`, `margin` → `Rect { top, right, bottom, left }` in points
   - `width`, `height` → `Dimension::Points(n)` or `Dimension::Auto`
   - `min_width`, `max_width`, etc. → `Dimension::Points(n)` or `Dimension::Auto`
   - `position: fixed` → `Position::Absolute` with `inset` values from top/left/right/bottom
4. For text nodes: set the size to the measured text dimensions (passed in as a parameter)
5. Call `taffy.compute_layout(root, available_space)`
6. Return a `Vec<LayoutResult>` with `(x, y, width, height)` per flat node

```rust
pub struct LayoutResult {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn compute_layout(
    flat_nodes: &[FlatNode],
    styles: &[ResolvedStyle],
    text_sizes: &[(f32, f32)], // (width, height) per node — (0,0) for non-text
    available_width: f32,
    available_height: f32,
) -> Vec<LayoutResult>
```

IMPORTANT: `taffy` API varies by version. The implementer MUST check the actual API:
- `taffy::TaffyTree::new()` or `Taffy::new()`
- `tree.new_leaf(style)` for leaf nodes, `tree.new_with_children(style, &children)` for containers
- `tree.compute_layout(root, available_space)`
- `tree.layout(node)` returns `&Layout` with `location.x`, `location.y`, `size.width`, `size.height`
- Location is relative to parent — convert to absolute by walking the tree

Tests:
- Vertical flex layout produces stacked positions
- Horizontal flex layout produces side-by-side positions
- `flex-grow` distributes remaining space
- `justify-content: center` centers children
- `position: fixed` uses absolute coordinates
- Text node sizes are respected

- [ ] **Step 2: Add module to mod.rs**

Add `pub mod layout;` to `host/src/omni/mod.rs`.

- [ ] **Step 3: Verify tests pass**

Run: `cargo test -p omni-host -- omni::layout`

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/layout.rs host/src/omni/mod.rs
git commit -m "feat(host): add taffy layout module for flexbox computation"
```

---

### Task 4: DirectWrite Text Measurement in Resolver

**Files:**
- Modify: `host/src/omni/resolver.rs`

Add `IDWriteFactory` to `OmniResolver` for measuring text dimensions.

- [ ] **Step 1: Add DirectWrite to OmniResolver**

```rust
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextLayout,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
};
use windows::core::w;

pub struct OmniResolver {
    theme_vars: HashMap<String, String>,
    dwrite_factory: Option<IDWriteFactory>,
}
```

Initialize in `new()`:

```rust
pub fn new() -> Self {
    let dwrite_factory = unsafe {
        DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok()
    };
    if dwrite_factory.is_none() {
        tracing::warn!("DirectWrite factory creation failed — text measurement unavailable");
    }
    Self {
        theme_vars: HashMap::new(),
        dwrite_factory,
    }
}
```

Add a `measure_text` method:

```rust
fn measure_text(&self, text: &str, font_size: f32, font_weight: u16) -> (f32, f32) {
    let factory = match &self.dwrite_factory {
        Some(f) => f,
        None => return (text.len() as f32 * font_size * 0.6, font_size + 4.0), // fallback
    };

    let weight = if font_weight >= 700 {
        DWRITE_FONT_WEIGHT_BOLD
    } else {
        DWRITE_FONT_WEIGHT_NORMAL
    };

    unsafe {
        let text_format = factory.CreateTextFormat(
            w!("Segoe UI"),
            None,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            font_size,
            w!("en-us"),
        );

        let text_format = match text_format {
            Ok(tf) => tf,
            Err(_) => return (text.len() as f32 * font_size * 0.6, font_size + 4.0),
        };

        let text_wide: Vec<u16> = text.encode_utf16().collect();
        let layout = factory.CreateTextLayout(
            &text_wide,
            &text_format,
            10000.0, // max width (we want natural width)
            10000.0, // max height
        );

        match layout {
            Ok(layout) => {
                match layout.GetMetrics() {
                    Ok(metrics) => (metrics.width, metrics.height),
                    Err(_) => (text.len() as f32 * font_size * 0.6, font_size + 4.0),
                }
            }
            Err(_) => (text.len() as f32 * font_size * 0.6, font_size + 4.0),
        }
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`

- [ ] **Step 3: Commit**

```bash
git add host/src/omni/resolver.rs
git commit -m "feat(host): add DirectWrite text measurement to OmniResolver"
```

---

### Task 5: Integrate Taffy into Resolver

**Files:**
- Modify: `host/src/omni/resolver.rs`

Replace manual position/size computation with taffy layout.

- [ ] **Step 1: Rewrite the resolve method**

The new flow:
1. Flatten tree (existing)
2. Resolve CSS for each non-text node (existing)
3. Collect text content and measure text for each text-bearing element
4. Call `layout::compute_layout(flat_nodes, styles, text_sizes, screen_w, screen_h)`
5. Use layout results for position/size instead of manual computation
6. Emit ComputedWidgets using layout positions

Remove:
- `positions` vec
- `parent_widths` vec
- Manual child stacking loop
- `estimate_flat_node_height` / `estimate_children_height_flat`

The key change in the main loop:

```rust
// Step 3: Measure text for text-bearing elements
let mut text_sizes: Vec<(f32, f32)> = vec![(0.0, 0.0); flat_nodes.len()];
for (i, node) in flat_nodes.iter().enumerate() {
    if !node.is_text && has_text_children(node, &flat_nodes) {
        let text = collect_text(node, &flat_nodes, snapshot);
        let font_size = parse_px(resolved_styles[i].as_ref()
            .and_then(|s| s.font_size.as_deref()))
            .unwrap_or(14.0);
        let font_weight = resolved_styles[i].as_ref()
            .and_then(|s| s.font_weight.as_deref())
            .and_then(|w| match w { "bold" => Some(700), _ => w.parse().ok() })
            .unwrap_or(400);
        text_sizes[i] = self.measure_text(&text, font_size, font_weight as u16);
    }
}

// Step 4: Compute layout with taffy
let resolved_for_layout: Vec<ResolvedStyle> = resolved_styles.iter()
    .map(|s| s.clone().unwrap_or_default())
    .collect();
let layouts = layout::compute_layout(
    &flat_nodes, &resolved_for_layout, &text_sizes,
    1920.0, 1080.0, // default screen size — TODO: get from config or DLL
);

// Step 5: Emit ComputedWidgets using layout positions
for (i, node) in flat_nodes.iter().enumerate() {
    if node.is_text { continue; }
    let layout = &layouts[i];
    // ... same widget emission but using layout.x, layout.y, layout.width, layout.height
}
```

- [ ] **Step 2: Update existing tests**

Existing resolver tests should still pass — the output positions may change slightly due to taffy's proper flex computation vs our manual approximation, but the tests should check for structural correctness (widget count, text content, source type) not exact pixel positions.

If position-sensitive tests break, update the expected values to match taffy's output.

- [ ] **Step 3: Verify all tests pass**

Run: `cargo test -p omni-host -- omni`

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/resolver.rs
git commit -m "feat(host): integrate taffy flexbox layout into resolver"
```

---

### Task 6: Parse Gradient and Box Shadow CSS Values

**Files:**
- Modify: `host/src/omni/css.rs`
- Modify: `host/src/omni/resolver.rs` (map parsed values to ComputedWidget fields)

- [ ] **Step 1: Add gradient parsing in resolver.rs**

In `style_to_computed_widget`, parse the `background` property for gradients:

```rust
// Parse gradient from background property
if let Some(bg) = &style.background {
    let bg_trimmed = bg.trim();
    if bg_trimmed.starts_with("linear-gradient(") {
        if let Some(gradient) = parse_linear_gradient(bg_trimmed) {
            cw.bg_gradient = gradient;
        }
    } else {
        cw.bg_color_rgba = parse_color(Some(bg_trimmed));
    }
}
// background-color as fallback
if cw.bg_color_rgba == [0, 0, 0, 0] {
    if let Some(bg_color) = &style.background_color {
        cw.bg_color_rgba = parse_color(Some(bg_color));
    }
}
```

Add `parse_linear_gradient` function:

```rust
fn parse_linear_gradient(value: &str) -> Option<omni_shared::GradientDef> {
    // linear-gradient(135deg, #ff0000, #0000ff)
    // linear-gradient(to right, red 0%, blue 100%)
    let inner = value.strip_prefix("linear-gradient(")?.strip_suffix(')')?;
    let parts: Vec<&str> = inner.splitn(3, ',').collect();
    if parts.len() < 2 { return None; }

    // Parse angle
    let angle_str = parts[0].trim();
    let angle_deg = if angle_str.ends_with("deg") {
        angle_str.trim_end_matches("deg").parse::<f32>().unwrap_or(180.0)
    } else if angle_str.starts_with("to ") {
        match angle_str {
            "to right" => 90.0,
            "to left" => 270.0,
            "to bottom" => 180.0,
            "to top" => 0.0,
            "to bottom right" => 135.0,
            "to top right" => 45.0,
            _ => 180.0,
        }
    } else {
        // First part might be a color (no angle specified, default 180deg)
        180.0
    };

    // Parse colors (simplified: first and last color stop)
    let color1_str = if angle_str.ends_with("deg") || angle_str.starts_with("to ") {
        parts.get(1).map(|s| s.trim()).unwrap_or("")
    } else {
        parts.get(0).map(|s| s.trim()).unwrap_or("")
    };
    let color2_str = parts.last().map(|s| s.trim()).unwrap_or("");

    // Strip percentage from color stops (e.g., "#ff0000 0%" → "#ff0000")
    let color1 = color1_str.split_whitespace().next().unwrap_or("");
    let color2 = color2_str.split_whitespace().next().unwrap_or("");

    Some(omni_shared::GradientDef {
        enabled: true,
        angle_deg,
        start_rgba: parse_color(Some(color1)),
        end_rgba: parse_color(Some(color2)),
    })
}
```

- [ ] **Step 2: Add box-shadow parsing**

```rust
fn parse_box_shadow(value: &str) -> Option<omni_shared::ShadowDef> {
    // box-shadow: 2px 4px 8px rgba(0,0,0,0.5)
    // box-shadow: 2px 4px 8px #000000
    let parts: Vec<&str> = value.trim().splitn(4, ' ').collect();
    if parts.len() < 3 { return None; }

    let offset_x = parse_px(Some(parts[0])).unwrap_or(0.0);
    let offset_y = parse_px(Some(parts[1])).unwrap_or(0.0);
    let blur_radius = parse_px(Some(parts[2])).unwrap_or(0.0);

    // Color is everything after the third value
    let color_str = if parts.len() >= 4 { parts[3] } else { "rgba(0,0,0,0.5)" };
    let color_rgba = parse_color(Some(color_str));

    Some(omni_shared::ShadowDef {
        enabled: true,
        offset_x,
        offset_y,
        blur_radius,
        color_rgba,
    })
}
```

Apply in `style_to_computed_widget`:

```rust
if let Some(shadow_str) = &style.box_shadow {
    if let Some(shadow) = parse_box_shadow(shadow_str) {
        cw.box_shadow = shadow;
    }
}
```

- [ ] **Step 3: Parse per-corner border-radius**

Update the border-radius parsing to handle 1-4 values:

```rust
fn parse_border_radius(value: &str) -> [f32; 4] {
    let parts: Vec<f32> = value.split_whitespace()
        .filter_map(|s| parse_px(Some(s)))
        .collect();

    match parts.len() {
        1 => [parts[0]; 4],
        2 => [parts[0], parts[1], parts[0], parts[1]], // top-left/bottom-right, top-right/bottom-left
        3 => [parts[0], parts[1], parts[2], parts[1]], // top-left, top-right/bottom-left, bottom-right
        4 => [parts[0], parts[1], parts[2], parts[3]], // all four
        _ => [0.0; 4],
    }
}
```

Apply in `style_to_computed_widget`:

```rust
if let Some(br) = &style.border_radius {
    cw.border_radius = parse_border_radius(br);
}
```

- [ ] **Step 4: Add tests**

Tests for gradient parsing:
- `linear-gradient(135deg, #ff0000, #0000ff)` → angle=135, start=red, end=blue
- `linear-gradient(to right, red, blue)` → angle=90
- No gradient → GradientDef default (disabled)

Tests for box-shadow parsing:
- `2px 4px 8px rgba(0,0,0,0.5)` → offset_x=2, offset_y=4, blur=8, color with 50% alpha
- `0 0 0` → zeroes

Tests for border-radius parsing:
- `8px` → all four corners 8
- `8px 0` → tl=8, tr=0, br=8, bl=0
- `8px 4px 2px 0` → each corner different

- [ ] **Step 5: Verify all tests pass**

Run: `cargo test -p omni-host -- omni`

- [ ] **Step 6: Commit**

```bash
git add host/src/omni/resolver.rs host/src/omni/css.rs
git commit -m "feat(host): parse linear-gradient, box-shadow, per-corner border-radius"
```

---

### Task 7: D2D Renderer — Gradient Brush

**Files:**
- Modify: `overlay-dll/src/renderer.rs`

Update the D2D renderer to draw linear gradients when `bg_gradient.enabled` is true.

- [ ] **Step 1: Add gradient rendering**

In the widget rendering loop, after the existing solid color background code, add gradient handling:

```rust
// Draw gradient background
if widget.bg_gradient.enabled {
    // Create gradient stops
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: D2D1_COLOR_F {
                r: widget.bg_gradient.start_rgba[0] as f32 / 255.0,
                g: widget.bg_gradient.start_rgba[1] as f32 / 255.0,
                b: widget.bg_gradient.start_rgba[2] as f32 / 255.0,
                a: (widget.bg_gradient.start_rgba[3] as f32 / 255.0) * widget.opacity,
            },
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: D2D1_COLOR_F {
                r: widget.bg_gradient.end_rgba[0] as f32 / 255.0,
                g: widget.bg_gradient.end_rgba[1] as f32 / 255.0,
                b: widget.bg_gradient.end_rgba[2] as f32 / 255.0,
                a: (widget.bg_gradient.end_rgba[3] as f32 / 255.0) * widget.opacity,
            },
        },
    ];

    // Compute start/end points from angle
    let (start_point, end_point) = gradient_points(
        &rect, widget.bg_gradient.angle_deg
    );

    // Create gradient brush
    if let Ok(stop_collection) = rt.CreateGradientStopCollection(&stops, ...) {
        let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
            startPoint: start_point,
            endPoint: end_point,
        };
        if let Ok(brush) = rt.CreateLinearGradientBrush(&props, None, &stop_collection) {
            rt.FillRectangle(&rect, &brush);
        }
    }
}
```

IMPORTANT: The D2D1 API for gradient stops and brushes varies in the `windows` crate. The implementer MUST check:
- `CreateGradientStopCollection` parameters
- `D2D1_GRADIENT_STOP` struct
- `D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES` struct
- `CreateLinearGradientBrush` parameters
- How to compute start/end points from an angle and a rect

Add a `gradient_points` helper that converts angle + rect into D2D1_POINT_2F start/end.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-overlay-dll`

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/src/renderer.rs
git commit -m "feat(overlay-dll): add linear gradient rendering via D2D1"
```

---

### Task 8: D2D Renderer — Box Shadow + Per-Corner Border Radius

**Files:**
- Modify: `overlay-dll/src/renderer.rs`

- [ ] **Step 1: Add box shadow rendering**

Before drawing the main background, draw the shadow. A simple approach:
- Create a solid color brush with the shadow color
- Offset the rect by (shadow.offset_x, shadow.offset_y)
- If blur_radius > 0: draw a slightly larger rect with reduced opacity to approximate blur
  (Full Gaussian blur via D2D1Effects is complex; a simple multi-pass approach with decreasing opacity at increasing offsets gives a reasonable approximation)

Simpler approach for v1: draw the shadow as a solid rect offset from the main rect, with the shadow color. Blur is approximated by drawing multiple slightly offset rects with decreasing opacity.

```rust
if widget.box_shadow.enabled {
    let shadow = &widget.box_shadow;
    let shadow_color = D2D1_COLOR_F { ... };
    let shadow_rect = D2D_RECT_F {
        left: rect.left + shadow.offset_x,
        top: rect.top + shadow.offset_y,
        right: rect.right + shadow.offset_x,
        bottom: rect.bottom + shadow.offset_y,
    };
    if let Ok(brush) = rt.CreateSolidColorBrush(&shadow_color, None) {
        rt.FillRoundedRectangle(&shadow_rounded, &brush);
    }
}
```

- [ ] **Step 2: Add per-corner border radius**

Replace the simplified `border_radius[0]` uniform radius with per-corner rendering using `ID2D1PathGeometry`:

```rust
fn draw_rounded_rect_per_corner(
    rt: &ID2D1RenderTarget,
    factory: &ID2D1Factory1,
    rect: &D2D_RECT_F,
    radii: [f32; 4], // [tl, tr, br, bl]
    brush: &ID2D1SolidColorBrush,
) {
    // If all corners are the same, use the simple API
    if radii[0] == radii[1] && radii[1] == radii[2] && radii[2] == radii[3] {
        if radii[0] > 0.0 {
            let rounded = D2D1_ROUNDED_RECT { rect: *rect, radiusX: radii[0], radiusY: radii[0] };
            unsafe { rt.FillRoundedRectangle(&rounded, brush); }
        } else {
            unsafe { rt.FillRectangle(rect, brush); }
        }
        return;
    }

    // Per-corner: build a path geometry with arcs
    // ... use ID2D1PathGeometry + ID2D1GeometrySink with ArcSegments
}
```

IMPORTANT: Building a path geometry with per-corner arcs requires:
- `factory.CreatePathGeometry()`
- `geometry.Open()` → `sink`
- `sink.BeginFigure(start_point, ...)`
- Lines and arcs for each corner
- `sink.EndFigure(...)` + `sink.Close()`
- `rt.FillGeometry(&geometry, brush, None)`

The implementer should check the D2D1 path geometry API for the exact method signatures.

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p omni-overlay-dll`

- [ ] **Step 4: Commit**

```bash
git add overlay-dll/src/renderer.rs
git commit -m "feat(overlay-dll): add box shadow and per-corner border radius rendering"
```

---

### Task 9: Integration Test — Flexbox + Visual Features

This is a manual integration test.

- [ ] **Step 1: Build everything**

```bash
cargo build -p omni-host && cargo build -p omni-overlay-dll
```

- [ ] **Step 2: Test flexbox layout**

Create an overlay with complex flex layout:

```xml
<widget id="test" name="Flex Test" enabled="true">
  <template>
    <div class="panel" style="position: fixed; top: 10px; left: 10px; width: 300px;">
      <div class="header">
        <span>System Monitor</span>
      </div>
      <div class="body">
        <div class="row">
          <span class="label">CPU</span>
          <span class="value">{cpu.usage}%</span>
        </div>
        <div class="row">
          <span class="label">GPU</span>
          <span class="value">{gpu.usage}%</span>
        </div>
      </div>
    </div>
  </template>
  <style>
    .panel { background: linear-gradient(180deg, rgba(30,30,50,0.9), rgba(10,10,20,0.9)); border-radius: 12px 12px 4px 4px; padding: 12px; display: flex; flex-direction: column; gap: 8px; box-shadow: 0 4px 12px rgba(0,0,0,0.5); }
    .header { display: flex; justify-content: center; }
    .header span { color: #44ff88; font-size: 18px; font-weight: bold; }
    .body { display: flex; flex-direction: column; gap: 4px; }
    .row { display: flex; flex-direction: row; justify-content: space-between; }
    .label { color: #888888; font-size: 14px; }
    .value { color: #ffffff; font-size: 14px; font-weight: bold; }
  </style>
</widget>
```

Verify:
- Panel has a gradient background (dark blue top to darker bottom)
- Box shadow visible below the panel
- Per-corner border radius (rounded top, less rounded bottom)
- "System Monitor" centered in header
- CPU/GPU rows with label on left, value on right (justify-content: space-between)

- [ ] **Step 3: Verify resilience**

Ctrl+C → restart → overlay renders correctly
widget.apply via WebSocket → flex layout updates live

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 9a-3 integration test"
```

---

## Phase 9a-3 Complete — Summary

At this point you have:

1. **Taffy flexbox** — full layout engine with `flex-direction`, `justify-content`, `align-items`, `flex-grow`, `flex-shrink`, `flex-wrap`, `gap`, `min/max` dimensions
2. **DirectWrite text measurement** — exact text width/height for accurate content sizing
3. **Linear gradients** — `background: linear-gradient(angle, color1, color2)` rendered via D2D
4. **Box shadow** — `box-shadow: x y blur color` rendered as offset shapes
5. **Per-corner border radius** — `border-radius: 8px 0 0 8px` via D2D path geometry
6. **Background shorthand** — `background` property parses into color or gradient
7. **Expanded flex properties** — `flex-grow`, `flex-shrink`, `flex-wrap`, `align-self`, `min/max` width/height

**Next:** Phase 9a-4 adds structured error reporting for Monaco integration.
