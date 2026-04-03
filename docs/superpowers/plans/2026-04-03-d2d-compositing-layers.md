# D2D Compositing Layers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add CSS-spec opacity compositing groups and per-axis `overflow: hidden` clipping to the D2D overlay renderer using Direct2D's `PushLayer`/`PopLayer` API.

**Architecture:** The host resolver emits `parent_index`, `overflow_x`, and `overflow_y` on each `ComputedWidget`. The DLL renderer pre-scans widgets to identify "layer parents" (opacity < 1.0 with children, or overflow hidden on either axis), then uses D2D `PushLayer`/`PopLayer` to composite subtrees as single visual units. This fixes overlapping-children transparency artifacts and enables content clipping.

**Tech Stack:** Rust, Direct2D (`ID2D1RenderTarget::PushLayer`/`PopLayer`), taffy (overflow), `#[repr(C)]` shared memory IPC.

**Spec:** `docs/superpowers/specs/2026-04-02-d2d-compositing-layers-design.md`

---

### Task 1: Add `parent_index`, `overflow_x`, `overflow_y` to `ComputedWidget`

**Files:**
- Modify: `shared/src/widget_types.rs:6-41` (ComputedWidget struct)
- Modify: `shared/src/widget_types.rs:136-172` (Default impl)
- Modify: `shared/src/ipc_protocol.rs:15` (IPC_PROTOCOL_VERSION)

- [ ] **Step 1: Write the failing test**

Add a test in `shared/src/widget_types.rs` inside the existing `mod tests` block:

```rust
#[test]
fn computed_widget_has_parent_and_overflow_fields() {
    let w = ComputedWidget::default();
    assert_eq!(w.parent_index, u16::MAX);
    assert_eq!(w.overflow_x, 0);
    assert_eq!(w.overflow_y, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p omni-shared computed_widget_has_parent_and_overflow_fields`
Expected: FAIL — `parent_index`, `overflow_x`, `overflow_y` don't exist on `ComputedWidget`.

- [ ] **Step 3: Add fields to `ComputedWidget` struct**

In `shared/src/widget_types.rs`, add three fields after `adaptive_dark_rgba`:

```rust
    pub adaptive_dark_rgba: [u8; 4],
    /// Index of parent widget in the widgets array. u16::MAX = no parent (root).
    pub parent_index: u16,
    /// Overflow behavior per axis: 0 = visible (default), 1 = hidden (clips children).
    pub overflow_x: u8,
    pub overflow_y: u8,
}
```

Update the `Default` impl to include:

```rust
            adaptive_dark_rgba: [0, 0, 0, 255],
            parent_index: u16::MAX,
            overflow_x: 0,
            overflow_y: 0,
```

- [ ] **Step 4: Bump `IPC_PROTOCOL_VERSION`**

In `shared/src/ipc_protocol.rs`, change:

```rust
pub const IPC_PROTOCOL_VERSION: u32 = 4;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p omni-shared`
Expected: All tests PASS including the new one.

- [ ] **Step 6: Commit**

```bash
git add shared/src/widget_types.rs shared/src/ipc_protocol.rs
git commit -m "feat: add parent_index, overflow_x, overflow_y to ComputedWidget"
```

---

### Task 2: Add `overflow_x`, `overflow_y` to `ResolvedStyle` and CSS resolver

**Files:**
- Modify: `host/src/omni/types.rs:68-119` (ResolvedStyle struct)
- Modify: `host/src/omni/css.rs:597-639` (props_to_resolved_style)

- [ ] **Step 1: Write the failing test**

Add a test in `host/src/omni/css.rs` inside the existing `mod tests` block:

```rust
#[test]
fn overflow_properties_resolved() {
    let (flat, _) = make_test_tree();
    let css = ".panel { overflow: hidden; }";
    let sheet = parse_css(css);

    let style = resolve_styles(&flat[0], 0, &flat, &sheet, &HashMap::new());
    assert_eq!(style.overflow.as_deref(), Some("hidden"));
}

#[test]
fn overflow_individual_axes_resolved() {
    let (flat, _) = make_test_tree();
    let css = ".panel { overflow-x: hidden; overflow-y: visible; }";
    let sheet = parse_css(css);

    let style = resolve_styles(&flat[0], 0, &flat, &sheet, &HashMap::new());
    assert_eq!(style.overflow_x.as_deref(), Some("hidden"));
    assert_eq!(style.overflow_y.as_deref(), Some("visible"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p omni-host overflow_properties_resolved overflow_individual_axes_resolved`
Expected: FAIL — `overflow`, `overflow_x`, `overflow_y` don't exist on `ResolvedStyle`.

- [ ] **Step 3: Add fields to `ResolvedStyle`**

In `host/src/omni/types.rs`, add after `transition`:

```rust
    pub transition: Option<String>,
    // Overflow
    pub overflow: Option<String>,
    pub overflow_x: Option<String>,
    pub overflow_y: Option<String>,
```

- [ ] **Step 4: Update `props_to_resolved_style`**

In `host/src/omni/css.rs`, in the `props_to_resolved_style` function, add after the `transition` line:

```rust
        transition: props.get("transition").cloned(),
        overflow: props.get("overflow").cloned(),
        overflow_x: props.get("overflow-x").cloned(),
        overflow_y: props.get("overflow-y").cloned(),
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p omni-host -- overflow`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add host/src/omni/types.rs host/src/omni/css.rs
git commit -m "feat: add overflow/overflow-x/overflow-y to ResolvedStyle and CSS resolver"
```

---

### Task 3: Add taffy overflow support in layout engine

**Files:**
- Modify: `host/src/omni/layout.rs:164-358` (build_taffy_style)

- [ ] **Step 1: Write the failing test**

Add a test in `host/src/omni/layout.rs` inside the existing `mod tests` block:

```rust
#[test]
fn overflow_hidden_sets_taffy_overflow() {
    // Verify that overflow: hidden is correctly mapped to taffy.
    // We test this indirectly: a child taller than its parent should
    // still report the parent's dimensions (taffy clips logical layout).
    let nodes = vec![
        elem("div", None, vec![1]),
        elem("div", Some(0), vec![]),
    ];
    let styles = vec![
        style_with(|s| {
            s.display = Some("flex".into());
            s.width = Some("100px".into());
            s.height = Some("50px".into());
            s.overflow = Some("hidden".into());
        }),
        style_with(|s| {
            s.width = Some("100px".into());
            s.height = Some("200px".into());
        }),
    ];
    let text_sizes = vec![(0.0, 0.0); 2];

    let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

    // Parent should be 50px tall (overflow doesn't expand)
    assert_eq!(results[0].height, 50.0);
    // Child is still 200px — taffy doesn't clip child size, just prevents
    // parent from growing. The D2D renderer handles visual clipping.
    assert_eq!(results[1].height, 200.0);
}

#[test]
fn overflow_per_axis() {
    let nodes = vec![
        elem("div", None, vec![1]),
        elem("div", Some(0), vec![]),
    ];
    let styles = vec![
        style_with(|s| {
            s.display = Some("flex".into());
            s.width = Some("100px".into());
            s.height = Some("50px".into());
            s.overflow_x = Some("hidden".into());
            s.overflow_y = Some("visible".into());
        }),
        style_with(|s| {
            s.width = Some("200px".into());
            s.height = Some("200px".into());
        }),
    ];
    let text_sizes = vec![(0.0, 0.0); 2];

    let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

    // Parent stays at its declared size
    assert_eq!(results[0].width, 100.0);
    assert_eq!(results[0].height, 50.0);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p omni-host overflow_hidden_sets_taffy_overflow overflow_per_axis`
Expected: FAIL — `overflow`, `overflow_x`, `overflow_y` not handled in `build_taffy_style`.

- [ ] **Step 3: Add overflow mapping to `build_taffy_style`**

In `host/src/omni/layout.rs`, add the following after the margin individual-side overrides block (after line ~320) and before the `// Size (width / height)` section:

```rust
    // Overflow — shorthand sets both axes, individual axes override
    if let Some(ref overflow) = style.overflow {
        let val = match overflow.as_str() {
            "hidden" => taffy::Overflow::Hidden,
            _ => taffy::Overflow::Visible,
        };
        ts.overflow = Point { x: val, y: val };
    }
    if let Some(ref ox) = style.overflow_x {
        ts.overflow.x = match ox.as_str() {
            "hidden" => taffy::Overflow::Hidden,
            _ => taffy::Overflow::Visible,
        };
    }
    if let Some(ref oy) = style.overflow_y {
        ts.overflow.y = match oy.as_str() {
            "hidden" => taffy::Overflow::Hidden,
            _ => taffy::Overflow::Visible,
        };
    }
```

Add `Point` to the taffy import at the top of the file. The existing import line is:

```rust
use taffy::prelude::*;
```

`Point` is already part of `taffy::prelude::*`, so no import change needed.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p omni-host -- overflow`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add host/src/omni/layout.rs
git commit -m "feat: map overflow/overflow-x/overflow-y to taffy layout engine"
```

---

### Task 4: Emit `parent_index` and overflow flags in the resolver

**Files:**
- Modify: `host/src/omni/resolver.rs:290-386` (widget emission loop in `resolve()`)

- [ ] **Step 1: Write the failing test**

Add a test in `host/src/omni/resolver.rs`. First, check if there's a `#[cfg(test)] mod tests` block at the bottom of the file. If not, create one. The test needs an OmniFile with nested elements and overflow:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::omni::types::{HtmlNode, OmniFile, Widget};
    use std::collections::HashMap;

    fn make_snapshot() -> SensorSnapshot {
        SensorSnapshot::default()
    }

    #[test]
    fn parent_index_emitted_for_nested_widgets() {
        let file = OmniFile {
            theme_src: None,
            poll_config: HashMap::new(),
            widgets: vec![Widget {
                id: "test".to_string(),
                name: "Test".to_string(),
                enabled: true,
                template: HtmlNode::Element {
                    tag: "div".to_string(),
                    id: None,
                    classes: vec!["parent".to_string()],
                    inline_style: None,
                    conditional_classes: vec![],
                    children: vec![HtmlNode::Element {
                        tag: "div".to_string(),
                        id: None,
                        classes: vec!["child".to_string()],
                        inline_style: None,
                        conditional_classes: vec![],
                        children: vec![HtmlNode::Text {
                            content: "hello".to_string(),
                        }],
                    }],
                },
                style_source: ".parent { background: rgba(0,0,0,0.5); } .child { color: white; }".to_string(),
            }],
        };

        let mut resolver = OmniResolver::new();
        let snap = make_snapshot();
        let widgets = resolver.resolve(&file, &snap);

        // Should emit at least 2 widgets (parent group + child text)
        assert!(widgets.len() >= 2, "Expected >= 2 widgets, got {}", widgets.len());

        // First widget should have parent_index = u16::MAX (root)
        assert_eq!(widgets[0].parent_index, u16::MAX);

        // Second widget should reference first as parent
        assert_eq!(widgets[1].parent_index, 0);
    }

    #[test]
    fn overflow_flags_emitted() {
        let file = OmniFile {
            theme_src: None,
            poll_config: HashMap::new(),
            widgets: vec![Widget {
                id: "test".to_string(),
                name: "Test".to_string(),
                enabled: true,
                template: HtmlNode::Element {
                    tag: "div".to_string(),
                    id: None,
                    classes: vec!["clip".to_string()],
                    inline_style: None,
                    conditional_classes: vec![],
                    children: vec![HtmlNode::Text {
                        content: "hello".to_string(),
                    }],
                },
                style_source: ".clip { overflow-x: hidden; overflow-y: visible; background: black; }".to_string(),
            }],
        };

        let mut resolver = OmniResolver::new();
        let snap = make_snapshot();
        let widgets = resolver.resolve(&file, &snap);

        assert!(!widgets.is_empty(), "Expected at least 1 widget");
        assert_eq!(widgets[0].overflow_x, 1, "overflow_x should be 1 (hidden)");
        assert_eq!(widgets[0].overflow_y, 0, "overflow_y should be 0 (visible)");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p omni-host parent_index_emitted overflow_flags_emitted`
Expected: FAIL — resolver doesn't set `parent_index` or `overflow_x`/`overflow_y` on emitted widgets.

- [ ] **Step 3: Build flat_node→widget index mapping and emit parent_index**

In `host/src/omni/resolver.rs`, inside the `resolve()` method, before the "Step 5: Emit ComputedWidgets" loop (around line 291), add a HashMap to track which flat node indices got emitted as which widget indices:

```rust
            // Build mapping from flat_node index → emitted widget index
            // so we can set parent_index on child widgets.
            let widget_base = widgets.len();
            let mut flat_to_widget: HashMap<usize, u16> = HashMap::new();
```

Then, inside the emission loop, after each `widgets.push(cw)`, record the mapping and set `parent_index` + overflow. Replace each occurrence of `widgets.push(cw)` with the pattern:

```rust
                    // Set parent_index: walk up flat tree to find nearest emitted ancestor
                    cw.parent_index = find_emitted_parent(
                        &flat_nodes, i, &flat_to_widget
                    );
                    // Set overflow flags from resolved style
                    set_overflow_flags(&mut cw, style);

                    let widget_idx = widgets.len() - widget_base;
                    flat_to_widget.insert(i, widget_idx as u16);
                    widgets.push(cw);
```

Add these two helper functions before the `style_to_computed_widget` function (outside `impl OmniResolver`):

```rust
/// Walk up the flat tree from `node_index` to find the nearest ancestor
/// that was emitted as a ComputedWidget. Returns u16::MAX if none found.
fn find_emitted_parent(
    flat_nodes: &[FlatNode],
    node_index: usize,
    flat_to_widget: &HashMap<usize, u16>,
) -> u16 {
    let mut current = flat_nodes[node_index].parent_index;
    while let Some(pi) = current {
        if let Some(&widget_idx) = flat_to_widget.get(&pi) {
            return widget_idx;
        }
        current = flat_nodes[pi].parent_index;
    }
    u16::MAX
}

/// Set overflow_x and overflow_y on a ComputedWidget from its ResolvedStyle.
fn set_overflow_flags(cw: &mut ComputedWidget, style: &ResolvedStyle) {
    // Shorthand sets both axes
    if let Some(ref overflow) = style.overflow {
        let val = if overflow == "hidden" { 1 } else { 0 };
        cw.overflow_x = val;
        cw.overflow_y = val;
    }
    // Individual axes override
    if let Some(ref ox) = style.overflow_x {
        cw.overflow_x = if ox == "hidden" { 1 } else { 0 };
    }
    if let Some(ref oy) = style.overflow_y {
        cw.overflow_y = if oy == "hidden" { 1 } else { 0 };
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p omni-host parent_index_emitted overflow_flags_emitted`
Expected: PASS.

- [ ] **Step 5: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests PASS.

- [ ] **Step 6: Commit**

```bash
git add host/src/omni/resolver.rs
git commit -m "feat: emit parent_index and overflow flags on ComputedWidgets"
```

---

### Task 5: Hierarchy-aware render loop with PushLayer/PopLayer in the DLL

**Files:**
- Modify: `overlay-dll/src/renderer.rs:572-831` (the `render()` method)

This is the core change. The current render loop at lines 604-826 is a flat `for widget in widgets` loop that draws each widget independently with opacity baked into brush alpha. We replace it with a hierarchy-aware loop that uses D2D `PushLayer`/`PopLayer` for compositing groups.

- [ ] **Step 1: Add D2D layer imports**

At the top of `overlay-dll/src/renderer.rs`, add to the existing `windows::Win32::Graphics::Direct2D` import block:

```rust
use windows::Win32::Graphics::Direct2D::{
    // ... existing imports ...
    D2D1_LAYER_OPTIONS_NONE, D2D1_LAYER_PARAMETERS,
};
```

Also add `ID2D1Layer` to the import if needed — but `PushLayer` accepts `None` for the layer parameter, so we can use `Option::<&ID2D1Layer>::None` and skip the import.

- [ ] **Step 2: Write the layer parent pre-scan function**

Add this function before the `render()` method:

```rust
/// Pre-scan widgets to identify "layer parents" — widgets that need a D2D PushLayer.
/// A widget is a layer parent if:
/// - opacity < 1.0 AND at least one other widget references it as parent_index
/// - OR overflow_x == 1 or overflow_y == 1
///
/// Returns a Vec<bool> parallel to widgets indicating which are layer parents,
/// and a Vec<u16> with the count of direct children for each widget.
fn scan_layer_parents(widgets: &[ComputedWidget]) -> (Vec<bool>, Vec<u16>) {
    let len = widgets.len();
    let mut is_layer_parent = vec![false; len];
    let mut child_count = vec![0u16; len];

    // Count children per parent
    for (i, w) in widgets.iter().enumerate() {
        let pi = w.parent_index as usize;
        if pi < len {
            child_count[pi] += 1;
        }
    }

    for (i, w) in widgets.iter().enumerate() {
        // Overflow clipping on either axis
        if w.overflow_x == 1 || w.overflow_y == 1 {
            is_layer_parent[i] = true;
            continue;
        }
        // Opacity compositing: only if has children (otherwise no compositing needed)
        if w.opacity < 1.0 && child_count[i] > 0 {
            is_layer_parent[i] = true;
        }
    }

    (is_layer_parent, child_count)
}
```

- [ ] **Step 3: Rewrite the render loop**

Replace the body of the `render()` method from `rt.BeginDraw()` through `rt.EndDraw()` (lines ~602-828) with the new hierarchy-aware loop. The key changes:

1. Pre-scan for layer parents
2. Maintain a stack of active layers (parent widget index, remaining child count)
3. PushLayer when entering a layer parent
4. PopLayer when all children of a layer parent have been drawn
5. When inside a layer, draw backgrounds/text at full opacity (the layer handles the fade)

```rust
        rt.BeginDraw();

        // Pre-scan for layer parents
        let (is_layer_parent, child_count) = scan_layer_parents(widgets);

        // Stack of active layers: (widget_index, remaining_children_to_draw)
        let mut layer_stack: Vec<(usize, u16)> = Vec::new();

        for (wi, widget) in widgets.iter().enumerate() {
            // Skip fully invisible widgets
            if widget.opacity < 0.001 {
                // Still need to account for this widget as a child of its parent layer
                if let Some(top) = layer_stack.last_mut() {
                    if widget.parent_index as usize == top.0
                        || is_descendant_of(widgets, wi, top.0)
                    {
                        // Count this as processed for child tracking
                    }
                }
                continue;
            }

            let rect = D2D_RECT_F {
                left: widget.x,
                top: widget.y,
                right: widget.x + widget.width,
                bottom: widget.y + widget.height,
            };

            let radii = widget.border_radius;

            // Determine if we're inside a compositing layer
            let in_layer = !layer_stack.is_empty();

            // Determine the effective opacity for this widget's drawing.
            // If this widget IS a layer parent, its own visual elements draw
            // at full opacity because the layer composites the group.
            // If this widget is INSIDE a layer but is not itself a layer parent,
            // draw at its own opacity (the parent layer handles the parent's fade).
            let draw_opacity = if is_layer_parent[wi] {
                1.0 // Layer handles compositing
            } else {
                widget.opacity
            };

            // If this widget is a layer parent, push a D2D layer
            if is_layer_parent[wi] {
                // Build content bounds for clipping
                let content_bounds = D2D_RECT_F {
                    left: if widget.overflow_x == 1 { widget.x } else { f32::MIN / 2.0 },
                    top: if widget.overflow_y == 1 { widget.y } else { f32::MIN / 2.0 },
                    right: if widget.overflow_x == 1 { widget.x + widget.width } else { f32::MAX / 2.0 },
                    bottom: if widget.overflow_y == 1 { widget.y + widget.height } else { f32::MAX / 2.0 },
                };

                let layer_opacity = if widget.opacity < 1.0 { widget.opacity } else { 1.0 };

                let layer_params = D2D1_LAYER_PARAMETERS {
                    contentBounds: content_bounds,
                    geometricMask: None,
                    maskAntialiasMode: windows::Win32::Graphics::Direct2D::D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                    maskTransform: windows::Win32::Graphics::Direct2D::Common::D2D_MATRIX_3X2_F {
                        M11: 1.0, M12: 0.0,
                        M21: 0.0, M22: 1.0,
                        M31: 0.0, M32: 0.0,
                    },
                    opacity: layer_opacity,
                    opacityBrush: None,
                    layerOptions: D2D1_LAYER_OPTIONS_NONE,
                };

                rt.PushLayer(&layer_params, None);
                layer_stack.push((wi, child_count[wi]));
            }

            // --- Draw box shadow ---
            if widget.box_shadow.enabled {
                let shadow = &widget.box_shadow;
                let sc = &shadow.color_rgba;
                let shadow_alpha = (sc[3] as f32 / 255.0) * draw_opacity;

                if shadow_alpha > 0.0 {
                    let shadow_rect = D2D_RECT_F {
                        left: rect.left + shadow.offset_x,
                        top: rect.top + shadow.offset_y,
                        right: rect.right + shadow.offset_x,
                        bottom: rect.bottom + shadow.offset_y,
                    };

                    let blur = shadow.blur_radius.max(0.0);
                    if blur > 0.0 {
                        let num_passes = ((blur / 2.0).ceil() as u32).clamp(4, 20);
                        for pass in (0..num_passes).rev() {
                            let t = (pass as f32 + 1.0) / num_passes as f32;
                            let expand = blur * t;
                            let alpha_factor = (-3.0 * t * t).exp();
                            let pass_alpha = shadow_alpha * alpha_factor / num_passes as f32;

                            let pass_rect = D2D_RECT_F {
                                left: shadow_rect.left - expand,
                                top: shadow_rect.top - expand,
                                right: shadow_rect.right + expand,
                                bottom: shadow_rect.bottom + expand,
                            };

                            let shadow_color = D2D1_COLOR_F {
                                r: sc[0] as f32 / 255.0,
                                g: sc[1] as f32 / 255.0,
                                b: sc[2] as f32 / 255.0,
                                a: pass_alpha,
                            };

                            let pass_radii = [
                                radii[0] + expand,
                                radii[1] + expand,
                                radii[2] + expand,
                                radii[3] + expand,
                            ];

                            if let Ok(shadow_brush) = rt.CreateSolidColorBrush(&shadow_color, None) {
                                fill_rounded_rect_per_corner(
                                    rt, &self.d2d_factory, &pass_rect, pass_radii, &shadow_brush,
                                );
                            }
                        }
                    } else {
                        let shadow_color = D2D1_COLOR_F {
                            r: sc[0] as f32 / 255.0,
                            g: sc[1] as f32 / 255.0,
                            b: sc[2] as f32 / 255.0,
                            a: shadow_alpha,
                        };
                        if let Ok(shadow_brush) = rt.CreateSolidColorBrush(&shadow_color, None) {
                            fill_rounded_rect_per_corner(
                                rt, &self.d2d_factory, &shadow_rect, radii, &shadow_brush,
                            );
                        }
                    }
                }
            }

            // --- Draw background ---
            if widget.bg_gradient.enabled {
                let grad = &widget.bg_gradient;
                let start_color = D2D1_COLOR_F {
                    r: grad.start_rgba[0] as f32 / 255.0,
                    g: grad.start_rgba[1] as f32 / 255.0,
                    b: grad.start_rgba[2] as f32 / 255.0,
                    a: (grad.start_rgba[3] as f32 / 255.0) * draw_opacity,
                };
                let end_color = D2D1_COLOR_F {
                    r: grad.end_rgba[0] as f32 / 255.0,
                    g: grad.end_rgba[1] as f32 / 255.0,
                    b: grad.end_rgba[2] as f32 / 255.0,
                    a: (grad.end_rgba[3] as f32 / 255.0) * draw_opacity,
                };

                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: start_color },
                    D2D1_GRADIENT_STOP { position: 1.0, color: end_color },
                ];

                if let Ok(stop_collection) =
                    rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)
                {
                    let (start_pt, end_pt) = gradient_points(&rect, grad.angle_deg);
                    let grad_props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: start_pt,
                        endPoint: end_pt,
                    };

                    if let Ok(brush) =
                        rt.CreateLinearGradientBrush(&grad_props, None, &stop_collection)
                    {
                        fill_rounded_rect_per_corner(rt, &self.d2d_factory, &rect, radii, &brush);
                    }
                }
            } else {
                let bg = &widget.bg_color_rgba;
                if bg[3] > 0 {
                    let bg_color = D2D1_COLOR_F {
                        r: bg[0] as f32 / 255.0,
                        g: bg[1] as f32 / 255.0,
                        b: bg[2] as f32 / 255.0,
                        a: (bg[3] as f32 / 255.0) * draw_opacity,
                    };

                    if let Ok(brush) = rt.CreateSolidColorBrush(&bg_color, None) {
                        fill_rounded_rect_per_corner(rt, &self.d2d_factory, &rect, radii, &brush);
                    }
                }
            }

            // --- Draw text ---
            let text = read_fixed_str(&widget.format_pattern);
            if !text.is_empty() {
                let font_weight = if widget.font_weight >= 700 {
                    DWRITE_FONT_WEIGHT_BOLD
                } else {
                    DWRITE_FONT_WEIGHT_NORMAL
                };

                let font_family_str = omni_shared::read_fixed_str(&widget.font_family);
                let font_family_wide: Vec<u16> = font_family_str
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();

                let text_format = self.dwrite_factory.CreateTextFormat(
                    windows::core::PCWSTR(font_family_wide.as_ptr()),
                    None,
                    font_weight,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    widget.font_size,
                    w!("en-us"),
                );

                if let Ok(text_format) = text_format {
                    let _ = text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                    let _ = text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);

                    let fg = &widget.color_rgba;
                    let fg_color = D2D1_COLOR_F {
                        r: fg[0] as f32 / 255.0,
                        g: fg[1] as f32 / 255.0,
                        b: fg[2] as f32 / 255.0,
                        a: (fg[3] as f32 / 255.0) * draw_opacity,
                    };

                    if let Ok(brush) = rt.CreateSolidColorBrush(&fg_color, None) {
                        let text_wide: Vec<u16> = text.encode_utf16().collect();
                        rt.DrawText(
                            &text_wide,
                            &text_format,
                            &rect,
                            &brush,
                            D2D1_DRAW_TEXT_OPTIONS_NONE,
                            DWRITE_MEASURING_MODE_NATURAL,
                        );
                    }
                } else if let Err(e) = text_format {
                    log_to_file(&format!(
                        "[renderer] CreateTextFormat failed for font '{}': {e}",
                        font_family_str
                    ));
                }
            }

            // --- Pop layers whose children are all drawn ---
            // After drawing widget wi, check if any active layers are complete.
            // A layer is complete when all its descendant widgets have been processed.
            // Since widgets are parent-before-child ordered, we pop when the next
            // widget is not a descendant of the layer parent.
            while let Some(top) = layer_stack.last() {
                let layer_parent_idx = top.0;
                // Check if the NEXT widget (wi+1) is still a descendant of this layer parent
                let next_wi = wi + 1;
                if next_wi >= widgets.len()
                    || !is_descendant_of(widgets, next_wi, layer_parent_idx)
                {
                    rt.PopLayer();
                    layer_stack.pop();
                } else {
                    break;
                }
            }
        }

        // Pop any remaining layers (shouldn't happen if widget ordering is correct)
        for _ in &layer_stack {
            rt.PopLayer();
        }

        let _ = rt.EndDraw(None, None);
```

- [ ] **Step 4: Add the `is_descendant_of` helper**

Add this function near `scan_layer_parents`:

```rust
/// Check if widget at `child_idx` is a descendant of widget at `ancestor_idx`.
/// Walks up the parent_index chain.
fn is_descendant_of(widgets: &[ComputedWidget], child_idx: usize, ancestor_idx: usize) -> bool {
    let mut current = child_idx;
    loop {
        let pi = widgets[current].parent_index as usize;
        if pi == ancestor_idx {
            return true;
        }
        if pi >= widgets.len() || pi == current {
            return false;
        }
        current = pi;
    }
}
```

- [ ] **Step 5: Build the DLL to verify compilation**

Run: `cargo build -p overlay-dll`
Expected: Compiles successfully. (DLL rendering can only be tested in-game, not via unit tests.)

- [ ] **Step 6: Commit**

```bash
git add overlay-dll/src/renderer.rs
git commit -m "feat: hierarchy-aware render loop with D2D PushLayer/PopLayer for opacity compositing and overflow clipping"
```

---

### Task 6: Regenerate TypeScript types and verify full build

**Files:**
- Modify: `desktop/src/generated/ResolvedStyle.ts` (auto-generated by ts-rs)

- [ ] **Step 1: Regenerate TypeScript types**

Run: `cargo test -p omni-host export_bindings`
Expected: TypeScript types regenerated in `desktop/src/generated/`.

- [ ] **Step 2: Verify the generated ResolvedStyle includes overflow fields**

Check that `desktop/src/generated/ResolvedStyle.ts` contains:
```typescript
overflow: string | null;
overflow_x: string | null;
overflow_y: string | null;
```

- [ ] **Step 3: Full workspace build**

Run: `cargo build --workspace`
Expected: All crates compile.

- [ ] **Step 4: Full test suite**

Run: `cargo test --workspace`
Expected: All tests PASS.

- [ ] **Step 5: Commit any generated type changes**

```bash
git add desktop/src/generated/
git commit -m "chore: regenerate TypeScript types for overflow fields"
```

---

## Summary of Changes

| File | Change |
|------|--------|
| `shared/src/widget_types.rs` | Add `parent_index: u16`, `overflow_x: u8`, `overflow_y: u8` to `ComputedWidget` |
| `shared/src/ipc_protocol.rs` | Bump `IPC_PROTOCOL_VERSION` to 4 |
| `host/src/omni/types.rs` | Add `overflow`, `overflow_x`, `overflow_y` to `ResolvedStyle` |
| `host/src/omni/css.rs` | Map `overflow`/`overflow-x`/`overflow-y` CSS properties to `ResolvedStyle` |
| `host/src/omni/layout.rs` | Map overflow values to taffy `Overflow::Hidden`/`Visible` |
| `host/src/omni/resolver.rs` | Emit `parent_index` + `overflow_x`/`overflow_y` on `ComputedWidget`; add `find_emitted_parent` + `set_overflow_flags` helpers |
| `overlay-dll/src/renderer.rs` | Replace flat render loop with hierarchy-aware loop using `PushLayer`/`PopLayer`; add `scan_layer_parents`, `is_descendant_of` helpers |
| `desktop/src/generated/*.ts` | Regenerated TypeScript types |
