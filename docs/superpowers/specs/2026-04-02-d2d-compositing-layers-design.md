# D2D Compositing Layers: Opacity Groups + Overflow Clipping

## Overview

Add CSS-spec opacity compositing and `overflow: hidden` clipping to the D2D overlay renderer using Direct2D's `PushLayer`/`PopLayer` API. Currently the renderer draws each widget independently with opacity baked into brush alpha, causing overlapping children to show through each other. This spec adds parent-child awareness so subtrees composite as a single visual unit.

## Problem

When a parent element has `opacity: 0.6` and overlapping children, each child is individually drawn at 60% opacity. Where children overlap, transparency compounds — the background shows through twice, creating visible seams. CSS specifies that `opacity` creates a compositing group: the subtree renders at full opacity first, then the entire group is faded as one unit.

Additionally, `overflow: hidden` is not supported. Overlay elements that extend beyond their parent's bounds are not clipped, preventing progress bars, slide-in animations, and content masking.

## Solution

Use Direct2D's `PushLayer`/`PopLayer` to create compositing groups. A D2D layer combines opacity and geometric clipping in one call. The renderer pushes a layer when a parent widget has `opacity < 1.0` or `overflow: hidden`, draws all children inside the layer, then pops it.

## Changes

### 1. `ComputedWidget` (shared/src/widget_types.rs)

Add two fields:

```rust
/// Index of parent widget in the widgets array. u16::MAX = no parent (root).
pub parent_index: u16,
/// Overflow behavior per axis: 0 = visible (default), 1 = hidden (clips children).
pub overflow_x: u8,
pub overflow_y: u8,
```

Default: `parent_index: u16::MAX`, `overflow_x: 0`, `overflow_y: 0`.

Bump `IPC_PROTOCOL_VERSION` to 4.

### 2. Host resolver (host/src/omni/resolver.rs)

**Emit `parent_index`:**
- The resolver already walks a flat tree where each `FlatNode` has `parent_index: Option<usize>`
- When emitting `ComputedWidget`s, maintain a mapping from flat node index → emitted widget index
- For each emitted widget, look up its flat node's `parent_index`, find the nearest ancestor that was also emitted as a `ComputedWidget`, and write that emitted index as `parent_index`
- If no ancestor was emitted, set `parent_index = u16::MAX`

**Emit `overflow_x` / `overflow_y`:**
- Add `overflow: Option<String>`, `overflow_x: Option<String>`, `overflow_y: Option<String>` to `ResolvedStyle`
- CSS resolver reads `overflow` (shorthand, sets both axes), `overflow-x`, `overflow-y` (individual overrides)
- Resolver maps `"hidden"` → `1`, everything else → `0` for each axis independently

**Emit order guarantee:**
The flat tree walk is top-down — parent flat nodes always have lower indices than children. The emission loop preserves this order. Parent widgets are always emitted before their children in the `widgets` Vec.

### 3. CSS resolver (host/src/omni/css.rs)

Add overflow properties:
```rust
overflow: props.get("overflow").cloned(),
overflow_x: props.get("overflow-x").cloned(),
overflow_y: props.get("overflow-y").cloned(),
```

`overflow` is the shorthand — sets both axes. `overflow-x` and `overflow-y` override individually.

### 4. Layout engine (host/src/omni/layout.rs)

Add taffy overflow support. Shorthand sets both, individual axes override:
```rust
// Shorthand
if let Some(ref overflow) = style.overflow {
    let val = match overflow.as_str() {
        "hidden" => taffy::Overflow::Hidden,
        _ => taffy::Overflow::Visible,
    };
    ts.overflow = Point { x: val, y: val };
}
// Individual overrides
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

### 5. DLL renderer (overlay-dll/src/renderer.rs)

**New render loop structure:**

Replace the flat `for widget in widgets` loop with a hierarchy-aware render:

```
1. Scan widgets to identify "layer parents" — widgets where:
   - opacity < 1.0 AND at least one other widget references them as parent_index
   - OR overflow_x == 1 or overflow_y == 1 (clipping on at least one axis)

2. Render in order (widgets are already parent-before-child):
   For each widget:
     a. If this widget is a layer parent:
        - Create D2D1_LAYER_PARAMETERS with:
          - opacity (if < 1.0, else 1.0)
          - contentBounds: per-axis clipping rect
            - overflow_x hidden: left = widget.x, right = widget.x + widget.width
            - overflow_x visible: left = f32::MIN, right = f32::MAX
            - overflow_y hidden: top = widget.y, bottom = widget.y + widget.height
            - overflow_y visible: top = f32::MIN, bottom = f32::MAX
        - PushLayer
        - Draw this widget's background/border/shadow at FULL opacity (layer handles fade)
     b. If this widget is NOT a layer parent:
        - Draw normally (opacity baked into brush alpha, as today)
     c. After drawing a widget, check if any previously pushed layers should be popped:
        - A layer should be popped when all children of the layer parent have been drawn
        - Track this with a stack: push (parent_index, child_count) on PushLayer,
          decrement child_count on each child drawn, PopLayer when count hits 0
```

**Layer pop tracking:**

Since widgets are emitted parent-before-child and the array is flat, we need to know when a parent's subtree ends. Two approaches:

**(A) Child count:** Pre-scan the widget array to count children per parent. After drawing the last child, pop the layer.

**(B) Next-non-descendant:** Track a stack of active layers. After drawing each widget, check if the NEXT widget in the array is NOT a descendant of the current layer parent. If so, pop.

Approach A is simpler. Pre-scan is O(n) with n ≤ 64.

**Opacity handling within layers:**

When inside a pushed layer:
- The parent's background/shadow/border are drawn with opacity = 1.0 (the layer composites)
- Children are drawn with their OWN opacity (could be 1.0 or their own value)
- If a child is itself a layer parent, it nests (PushLayer inside PushLayer)

When NOT inside a layer:
- Opacity is baked into brush alpha as today (backward compatible for simple cases)

### 6. Validation (host/src/omni/validation.rs)

`overflow` is already in `KNOWN_CSS_PROPERTIES`. No changes needed.

### 7. ResolvedStyle (host/src/omni/types.rs)

Add:
```rust
pub overflow: Option<String>,
pub overflow_x: Option<String>,
pub overflow_y: Option<String>,
```

## Files Modified

| File | Change |
|------|--------|
| `shared/src/widget_types.rs` | Add `parent_index: u16`, `overflow: u8` to `ComputedWidget` |
| `shared/src/ipc_protocol.rs` | Bump `IPC_PROTOCOL_VERSION` to 4 |
| `host/src/omni/types.rs` | Add `overflow: Option<String>` to `ResolvedStyle` |
| `host/src/omni/css.rs` | Read `overflow` property |
| `host/src/omni/layout.rs` | Map `overflow: hidden` to taffy |
| `host/src/omni/resolver.rs` | Emit `parent_index` + `overflow_x/overflow_y` on `ComputedWidget` |
| `overlay-dll/src/renderer.rs` | Hierarchy-aware render loop with `PushLayer`/`PopLayer` |

## Out of Scope

- `overflow: scroll` (no scrollbars in the overlay)
- `transform` compositing (`transform: scale()`, `rotate()`)
- `mix-blend-mode`
- `clip-path` (arbitrary clip shapes)
