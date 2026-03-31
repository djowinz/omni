# Phase 9b: CSS Transitions + Reactive Class Binding

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **IMPORTANT: Run superpowers:code-reviewer after EVERY subagent task. No exceptions.**

**Goal:** Add reactive class binding (`class:name="expression"`) driven by sensor data conditions, CSS transition interpolation with easing functions, and a 120Hz main loop for smooth animations.

**Architecture:** Four new modules. (1) Expression evaluator: tokenizer + recursive descent parser for math/comparison/logical expressions against sensor data. (2) Reactive class system: evaluates `class:name="expr"` attributes each frame, tracks state changes. (3) Transition engine: detects property value changes from class additions/removals, interpolates with easing functions over time. (4) Main loop refactored to tick at 120Hz with scanner throttled separately.

**Tech Stack:** Rust, `std::time::Instant` for animation timing.

**Testing notes:** Expression evaluator is fully unit-testable with synthetic sensor data. Transition interpolation testable with mock time. Class binding testable with known conditions. Full pipeline tested manually in-game.

**Depends on:** Phase 9a-3 complete (taffy flexbox, D2D visual features, lightningcss CSS).

---

## File Map

```
host/
  src/
    main.rs                          # Refactor: 120Hz loop, scanner throttle
    omni/
      mod.rs                         # Add new modules
      types.rs                       # Add ConditionalClass to HtmlNode
      parser.rs                      # Parse class:name="expr" attributes
      flat_tree.rs                   # Add conditional_classes to FlatNode
      resolver.rs                    # Integrate reactive classes + transitions
      expression.rs                  # NEW: tokenizer + recursive descent evaluator
      reactive.rs                    # NEW: conditional class evaluation + state tracking
      transition.rs                  # NEW: transition parsing, state tracking, interpolation
      easing.rs                      # NEW: easing functions (linear, ease, cubic-bezier)
```

---

### Task 1: Expression Evaluator

**Files:**
- Create: `host/src/omni/expression.rs`
- Modify: `host/src/omni/mod.rs` (add `pub mod expression;`)

A tokenizer + recursive descent parser that evaluates math/comparison/logical expressions against sensor data.

- [ ] **Step 1: Create host/src/omni/expression.rs**

The module needs:

**Tokenizer:** Converts input string into tokens:
- `Number(f64)` — numeric literals
- `SensorPath(String)` — dot-separated sensor paths like `gpu.temp`
- `Plus`, `Minus`, `Star`, `Slash` — math operators
- `Gt`, `Lt`, `Gte`, `Lte`, `Eq`, `Neq` — comparison operators
- `And`, `Or`, `Not` — logical operators
- `LParen`, `RParen` — grouping

**Recursive descent parser/evaluator:**
```
expr     → or_expr
or_expr  → and_expr ( "||" and_expr )*
and_expr → not_expr ( "&&" not_expr )*
not_expr → "!" not_expr | compare
compare  → add_expr ( (">" | "<" | ">=" | "<=" | "==" | "!=") add_expr )?
add_expr → mul_expr ( ("+" | "-") mul_expr )*
mul_expr → primary ( ("*" | "/") primary )*
primary  → NUMBER | SENSOR_PATH | "(" expr ")"
```

**Public API:**
```rust
/// Evaluate a condition expression against sensor data. Returns true/false.
/// Malformed expressions return false and log a warning (once per unique expr).
pub fn eval_condition(expr: &str, snapshot: &SensorSnapshot) -> bool

/// Evaluate an expression to a numeric value.
/// Used for interpolation targets (e.g., computing percentages).
pub fn eval_numeric(expr: &str, snapshot: &SensorSnapshot) -> Option<f64>
```

Sensor paths resolve via `sensor_map::get_sensor_value` parsed to `f64`. Unknown paths return `0.0`.

**Tests (at least 10):**
- Simple comparison: `"gpu.temp > 80"` with temp=85 → true
- Simple comparison: `"gpu.temp > 80"` with temp=70 → false
- Math expression: `"gpu.vram.used / gpu.vram.total > 0.9"` → evaluates correctly
- Logical AND: `"cpu.usage > 90 && gpu.usage > 90"` — both true, one false
- Logical OR: `"fps < 30 || gpu.temp > 95"` — first true, second true, neither
- NOT: `"!(fps > 60)"` with fps=30 → true
- Parentheses: `"(cpu.usage + gpu.usage) / 2 > 80"`
- Equality: `"fps == 60"`
- Nested parens: `"((gpu.temp > 80) && (cpu.usage > 90)) || fps < 30"`
- Invalid expression: `"invalid !!@ garbage"` → false (no panic)
- Division by zero: `"100 / 0 > 1"` → false (no panic)

- [ ] **Step 2: Add module to mod.rs**

Add `pub mod expression;` to `host/src/omni/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host -- omni::expression`
Expected: 10+ tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/expression.rs host/src/omni/mod.rs
git commit -m "feat(host): add expression evaluator for reactive class conditions"
```

---

### Task 2: Easing Functions

**Files:**
- Create: `host/src/omni/easing.rs`
- Modify: `host/src/omni/mod.rs` (add `pub mod easing;`)

Pure math functions for CSS easing curves.

- [ ] **Step 1: Create host/src/omni/easing.rs**

```rust
//! CSS easing functions for transition interpolation.

/// An easing function that maps progress (0.0–1.0) to an output value (0.0–1.0).
#[derive(Debug, Clone)]
pub enum EasingFunction {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier(f64, f64, f64, f64), // (x1, y1, x2, y2)
}

impl EasingFunction {
    /// Parse a CSS easing function name or cubic-bezier(...).
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "linear" => Self::Linear,
            "ease" => Self::Ease,
            "ease-in" => Self::EaseIn,
            "ease-out" => Self::EaseOut,
            "ease-in-out" => Self::EaseInOut,
            s if s.starts_with("cubic-bezier(") => {
                if let Some(inner) = s.strip_prefix("cubic-bezier(").and_then(|s| s.strip_suffix(')')) {
                    let parts: Vec<f64> = inner.split(',')
                        .filter_map(|p| p.trim().parse().ok())
                        .collect();
                    if parts.len() == 4 {
                        return Self::CubicBezier(parts[0], parts[1], parts[2], parts[3]);
                    }
                }
                Self::Ease // fallback
            }
            _ => Self::Ease, // default
        }
    }

    /// Apply the easing function to a progress value (0.0–1.0).
    /// Returns the eased output (0.0–1.0).
    pub fn apply(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
            Self::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
            Self::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
            Self::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
            Self::CubicBezier(x1, y1, x2, y2) => cubic_bezier(*x1, *y1, *x2, *y2, t),
        }
    }
}

/// Evaluate a cubic Bézier curve at progress t.
/// Uses Newton-Raphson to find the t parameter for the given x, then evaluates y.
fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64 {
    // Find the parameter value for the given t (x-axis progress)
    let mut guess = t;
    for _ in 0..8 {
        let x = bezier_component(x1, x2, guess) - t;
        if x.abs() < 1e-6 {
            break;
        }
        let dx = bezier_derivative(x1, x2, guess);
        if dx.abs() < 1e-6 {
            break;
        }
        guess -= x / dx;
    }
    bezier_component(y1, y2, guess)
}

fn bezier_component(p1: f64, p2: f64, t: f64) -> f64 {
    let t2 = t * t;
    let t3 = t2 * t;
    3.0 * (1.0 - t) * (1.0 - t) * t * p1 + 3.0 * (1.0 - t) * t2 * p2 + t3
}

fn bezier_derivative(p1: f64, p2: f64, t: f64) -> f64 {
    let t2 = t * t;
    6.0 * (1.0 - t) * t * (p2 - p1) + 3.0 * (1.0 - t) * (1.0 - t) * p1 + 3.0 * t2 * (1.0 - p2)
}
```

Wait — the bezier derivative formula above is wrong. The correct derivative of the cubic Bezier x(t) = 3(1-t)²t·p1 + 3(1-t)t²·p2 + t³ is:

dx/dt = 3(1-t)²·p1 + 6(1-t)t·(p2-p1) + 3t²·(1-p2)

Actually this is also not standard. Let the implementer derive the correct formulas or use a well-known implementation. The key is:
- `bezier_component(p1, p2, t)` evaluates the cubic Bezier `B(t) = 3(1-t)²t·p1 + 3(1-t)t²·p2 + t³`
- `bezier_derivative` is its derivative for Newton-Raphson root finding
- `cubic_bezier(x1, y1, x2, y2, progress)` finds `t` such that `B_x(t) = progress`, then returns `B_y(t)`

**Tests:**
- `Linear: t=0 → 0, t=0.5 → 0.5, t=1 → 1`
- `Ease: t=0 → 0, t=1 → 1, t=0.5 → ~0.8` (ease front-loads progress)
- `EaseIn: t=0.5 → < 0.5` (slow start)
- `EaseOut: t=0.5 → > 0.5` (fast start)
- `EaseInOut: t=0.5 → ~0.5` (symmetric)
- `Parse "ease" → Ease`
- `Parse "cubic-bezier(0.1, 0.2, 0.3, 0.4)" → CubicBezier`
- `Parse "unknown" → Ease (default)`

- [ ] **Step 2: Add module to mod.rs**

- [ ] **Step 3: Run tests**

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/easing.rs host/src/omni/mod.rs
git commit -m "feat(host): add CSS easing functions (linear, ease, cubic-bezier)"
```

---

### Task 3: Transition Engine

**Files:**
- Create: `host/src/omni/transition.rs`
- Modify: `host/src/omni/mod.rs` (add `pub mod transition;`)

Parses CSS `transition` property, tracks active transitions, interpolates property values over time.

- [ ] **Step 1: Create host/src/omni/transition.rs**

**Types:**

```rust
use std::collections::HashMap;
use std::time::Instant;
use super::easing::EasingFunction;

/// A parsed transition rule for a single property.
#[derive(Debug, Clone)]
pub struct TransitionRule {
    pub property: String,      // "width", "opacity", "background", "all"
    pub duration_ms: f64,      // 300.0
    pub delay_ms: f64,         // 0.0
    pub easing: EasingFunction,
}

/// An active transition being interpolated.
#[derive(Debug, Clone)]
struct ActiveTransition {
    property: String,
    from_value: String,
    to_value: String,
    start_time: Instant,
    duration_ms: f64,
    delay_ms: f64,
    easing: EasingFunction,
}

/// Manages transition state for all elements across all widgets.
pub struct TransitionManager {
    /// Key: (widget_id, element_index, property_name)
    active: HashMap<(String, usize, String), ActiveTransition>,
    /// Previous frame's resolved property values per element.
    /// Key: (widget_id, element_index) → HashMap<property, value>
    previous_values: HashMap<(String, usize), HashMap<String, String>>,
}
```

**Public API:**

```rust
impl TransitionManager {
    pub fn new() -> Self { ... }

    /// Parse a CSS `transition` property value into rules.
    /// e.g., "width 0.3s ease, background 0.3s ease-in-out 0.1s"
    pub fn parse_transition(value: &str) -> Vec<TransitionRule> { ... }

    /// Update transitions for an element. Call once per element per frame.
    /// - `widget_id`: the widget this element belongs to
    /// - `element_idx`: the element's index in the flat tree
    /// - `transition_rules`: parsed from the element's CSS `transition` property
    /// - `current_values`: the newly resolved CSS property values
    /// Returns: HashMap of property → interpolated value (overrides for this frame)
    pub fn update(
        &mut self,
        widget_id: &str,
        element_idx: usize,
        transition_rules: &[TransitionRule],
        current_values: &HashMap<String, String>,
    ) -> HashMap<String, String> { ... }
}
```

**`update` logic:**
1. Look up `previous_values` for this element
2. For each property in `current_values`, check if it changed from the previous frame
3. If changed and a `transition_rule` matches (by property name, or "all"):
   - Create an `ActiveTransition` with `from=previous, to=current, start=now`
4. For each active transition:
   - Compute `elapsed = now - start_time`
   - If `elapsed < delay`: return `from_value`
   - If `elapsed >= delay + duration`: transition complete, remove it
   - Otherwise: compute `t = (elapsed - delay) / duration`, apply easing, interpolate
5. Store `current_values` as the new `previous_values`
6. Return map of property → interpolated value

**Value interpolation:**
- Numeric values (e.g., "60px" → "200px"): lerp the number, keep the unit
- Color values (e.g., "rgba(20,20,20,0.7)" → "rgba(255,136,0,0.7)"): per-channel lerp
- Opacity (e.g., "0" → "1"): simple float lerp

Add a helper `interpolate_value(from: &str, to: &str, t: f64) -> String` that handles these cases.

**Tests:**
- Parse `"width 0.3s ease"` → 1 rule, duration=300, easing=Ease
- Parse `"width 0.3s ease, opacity 0.5s linear"` → 2 rules
- Parse `"all 0.3s ease-in-out"` → 1 rule with property="all"
- Parse `"width 0.3s ease 0.1s"` → delay=100ms
- Interpolate numeric: "60px" to "200px" at t=0.5 → "130px"
- Interpolate color: rgba channels lerp correctly
- Interpolate opacity: "0" to "1" at t=0.5 → "0.5"
- Update detects property change and starts transition
- Update returns interpolated value mid-transition
- Completed transition returns final value

- [ ] **Step 2: Add module to mod.rs**

- [ ] **Step 3: Run tests**

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/transition.rs host/src/omni/mod.rs
git commit -m "feat(host): add CSS transition engine with parsing and interpolation"
```

---

### Task 4: Reactive Class Binding — Types + Parser

**Files:**
- Modify: `host/src/omni/types.rs`
- Modify: `host/src/omni/parser.rs`
- Modify: `host/src/omni/flat_tree.rs`

Add `ConditionalClass` to the data model and parse `class:name="expr"` attributes.

- [ ] **Step 1: Add ConditionalClass to types.rs**

```rust
/// A conditional class binding: class is added when expression evaluates to true.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConditionalClass {
    pub class_name: String,
    pub expression: String,
}
```

Add to `HtmlNode::Element`:
```rust
HtmlNode::Element {
    tag: String,
    id: Option<String>,
    classes: Vec<String>,
    conditional_classes: Vec<ConditionalClass>,  // NEW
    inline_style: Option<String>,
    children: Vec<HtmlNode>,
}
```

- [ ] **Step 2: Update parser.rs to extract class:* attributes**

In `parse_html_element` and `parse_empty_html_element`, after extracting `class` and `style` attributes, scan for attributes starting with `class:`:

```rust
let mut conditional_classes = Vec::new();
for attr in start.attributes().filter_map(|a| a.ok()) {
    let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
    if let Some(class_name) = key.strip_prefix("class:") {
        let expression = String::from_utf8_lossy(&attr.value).to_string();
        conditional_classes.push(ConditionalClass {
            class_name: class_name.to_string(),
            expression,
        });
    }
}
```

Add `conditional_classes` to the `HtmlNode::Element` construction.

- [ ] **Step 3: Update flat_tree.rs to include conditional_classes**

Add `pub conditional_classes: Vec<ConditionalClass>` to `FlatNode`. Update `flatten_recursive` to copy conditional classes from `HtmlNode::Element`.

- [ ] **Step 4: Add tests**

```rust
#[test]
fn parse_conditional_classes() {
    let source = r#"
        <widget id="test" name="Test" enabled="true">
            <template>
                <div class="pill" class:warning="gpu.temp > 80" class:critical="gpu.temp > 95">
                    <span>{gpu.temp}°C</span>
                </div>
            </template>
            <style></style>
        </widget>
    "#;

    let file = parse_omni(source).unwrap();
    if let HtmlNode::Element { conditional_classes, .. } = &file.widgets[0].template {
        assert_eq!(conditional_classes.len(), 2);
        assert_eq!(conditional_classes[0].class_name, "warning");
        assert_eq!(conditional_classes[0].expression, "gpu.temp > 80");
        assert_eq!(conditional_classes[1].class_name, "critical");
        assert_eq!(conditional_classes[1].expression, "gpu.temp > 95");
    }
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test -p omni-host -- omni`
Expected: All tests pass (existing + new).

- [ ] **Step 6: Commit**

```bash
git add host/src/omni/types.rs host/src/omni/parser.rs host/src/omni/flat_tree.rs
git commit -m "feat(host): parse class:name=\"expr\" conditional class bindings"
```

---

### Task 5: Reactive Class System

**Files:**
- Create: `host/src/omni/reactive.rs`
- Modify: `host/src/omni/mod.rs` (add `pub mod reactive;`)

Evaluates conditional classes each frame and returns the active class list.

- [ ] **Step 1: Create host/src/omni/reactive.rs**

```rust
//! Reactive class evaluation — computes active classes per element each frame.

use omni_shared::SensorSnapshot;
use super::expression;
use super::flat_tree::FlatNode;

/// Evaluate all conditional classes for a flat node and return the full active class list
/// (static classes + conditionally active classes).
pub fn resolve_active_classes(node: &FlatNode, snapshot: &SensorSnapshot) -> Vec<String> {
    let mut active = node.classes.clone();

    for cc in &node.conditional_classes {
        if expression::eval_condition(&cc.expression, snapshot) {
            if !active.contains(&cc.class_name) {
                active.push(cc.class_name.clone());
            }
        }
        // If condition is false, the class is NOT in the list
        // (it was only in `active` if it was also a static class)
    }

    active
}
```

**Tests:**
- Element with no conditional classes → returns static classes only
- Condition true → class added
- Condition false → class not added
- Multiple conditions: some true, some false → correct subset
- Static class + same conditional class → no duplicates

- [ ] **Step 2: Add module to mod.rs**

- [ ] **Step 3: Run tests**

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/reactive.rs host/src/omni/mod.rs
git commit -m "feat(host): add reactive class evaluation from sensor conditions"
```

---

### Task 6: Integrate Everything into Resolver + Main Loop

**Files:**
- Modify: `host/src/omni/resolver.rs`
- Modify: `host/src/omni/types.rs` (add `transition` to ResolvedStyle)
- Modify: `host/src/omni/css.rs` (map `transition` property)
- Modify: `host/src/main.rs`

This is the integration task that wires reactive classes, transitions, and the 120Hz loop together.

- [ ] **Step 1: Add `transition` field to ResolvedStyle**

In `types.rs`, add:
```rust
pub transition: Option<String>,
```

In `css.rs`, add to `props_to_resolved_style`:
```rust
transition: props.get("transition").cloned(),
```

- [ ] **Step 2: Update OmniResolver to use reactive classes + transitions**

Add `TransitionManager` to `OmniResolver`:

```rust
pub struct OmniResolver {
    theme_vars: HashMap<String, String>,
    dwrite_factory: Option<IDWriteFactory>,
    transition_manager: transition::TransitionManager,  // NEW
}
```

In the resolve method, for each non-text element:
1. **Evaluate reactive classes:** Call `reactive::resolve_active_classes(node, snapshot)` to get the active class list
2. **Create a modified FlatNode** with the active classes for CSS resolution
3. **Resolve CSS** with the active classes → get property values
4. **Parse transition rules** from the resolved `transition` property
5. **Call `transition_manager.update()`** with current property values → get interpolated overrides
6. **Apply overrides** to the resolved style before layout/emission

The key change in the per-element loop:

```rust
// Evaluate reactive classes
let active_classes = reactive::resolve_active_classes(node, snapshot);

// Create modified node with active classes for CSS resolution
let mut resolve_node = node.clone();
resolve_node.classes = active_classes;
resolve_node.inline_style = interpolated_inline;

let mut style = css::resolve_styles(
    &resolve_node, i, &flat_nodes, &stylesheet, &self.theme_vars,
);

// Apply transitions
if let Some(transition_str) = &style.transition {
    let rules = transition::TransitionManager::parse_transition(transition_str);
    let current_props = style_to_property_map(&style);
    let overrides = self.transition_manager.update(
        &widget_def.id, i, &rules, &current_props,
    );
    apply_property_overrides(&mut style, &overrides);
}
```

Add helper functions:
- `style_to_property_map(style: &ResolvedStyle) -> HashMap<String, String>` — extracts animatable properties
- `apply_property_overrides(style: &mut ResolvedStyle, overrides: &HashMap<String, String>)` — applies interpolated values back

- [ ] **Step 3: Refactor main loop to 120Hz**

In `main.rs`, change the main loop:

```rust
let scan_interval = Duration::from_millis(2000);
let frame_interval = Duration::from_millis(8); // 120Hz
let mut last_scan = Instant::now();

while RUNNING.load(Ordering::Relaxed) {
    // Throttle scanner to scan_interval
    if last_scan.elapsed() >= scan_interval {
        scanner_instance.poll();
        last_scan = Instant::now();
    }

    // Drain sensor channel
    while let Ok(snapshot) = sensor_rx.try_recv() {
        latest_snapshot = snapshot;
    }

    // Update WebSocket shared state
    if let Ok(mut ws_snapshot) = ws_state.latest_snapshot.lock() {
        *ws_snapshot = latest_snapshot;
    }

    // Check for widget updates from WebSocket
    if let Ok(mut active) = ws_state.active_omni_file.lock() {
        if let Some(new_file) = active.take() {
            // ... existing overlay reload logic ...
            omni_file = new_file;
        }
    }

    // Resolve widgets (now includes reactive classes + transition interpolation)
    let widgets = omni_resolver.resolve(&omni_file, &latest_snapshot);
    shm_writer.write(&latest_snapshot, &widgets, 1);

    std::thread::sleep(frame_interval);
}
```

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add host/src/omni/resolver.rs host/src/omni/types.rs host/src/omni/css.rs host/src/main.rs
git commit -m "feat(host): integrate reactive classes + transitions + 120Hz main loop"
```

---

### Task 7: Integration Test — Reactive Transitions

This is a manual integration test.

- [ ] **Step 1: Build everything**

```cmd
cargo build -p omni-host && cargo build -p omni-overlay-dll
```

- [ ] **Step 2: Create test overlay**

Create `%APPDATA%\Omni\overlays\Default\overlay.omni`:

```xml
<widget id="monitor" name="System Monitor" enabled="true">
  <template>
    <div class="panel" style="position: fixed; top: 20px; left: 20px; width: 250px;"
         class:hot="gpu.temp > 70"
         class:critical="gpu.temp > 85">
      <span class="title">GPU Monitor</span>
      <div class="row">
        <span class="label">Temp</span>
        <span class="value temp-value">{gpu.temp}°C</span>
      </div>
      <div class="row">
        <span class="label">Usage</span>
        <span class="value">{gpu.usage}%</span>
      </div>
      <div class="row">
        <span class="label">FPS</span>
        <span class="value">{fps}</span>
      </div>
    </div>
  </template>
  <style>
    .panel {
      background: rgba(20, 30, 40, 0.85);
      border-radius: 8px;
      padding: 12px;
      display: flex;
      flex-direction: column;
      gap: 6px;
      transition: background 0.5s ease, width 0.3s ease;
    }
    .panel.hot {
      background: rgba(60, 40, 20, 0.85);
      width: 300px;
    }
    .panel.critical {
      background: rgba(80, 20, 20, 0.9);
    }
    .title { color: #44ff88; font-size: 16px; font-weight: bold; }
    .row { display: flex; flex-direction: row; justify-content: space-between; }
    .label { color: #888888; font-size: 14px; }
    .value { color: white; font-size: 14px; transition: color 0.3s ease; }
    .temp-value { transition: color 0.3s ease; }
    .hot .temp-value { color: #ff8844; }
    .critical .temp-value { color: #ff4444; }
  </style>
</widget>
```

- [ ] **Step 3: Test transitions**

Start the host and launch a game. As GPU temp changes:
- Below 70°C: dark blue-gray panel, white temp text, 250px wide
- Above 70°C: warm brown panel smoothly fading in over 0.5s, temp text turns orange, panel widens to 300px
- Above 85°C: dark red panel, temp text turns red
- When temp drops: smooth transition back to previous state

- [ ] **Step 4: Verify 120Hz smoothness**

The transitions should be smooth with no visible stepping. Compare against NVIDIA overlay for perceived smoothness.

- [ ] **Step 5: Verify resilience**

- Ctrl+C → restart → transitions work after reload
- widget.apply via WebSocket → reactive classes evaluate with new source

- [ ] **Step 6: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 9b integration test"
```

---

## Phase 9b Complete — Summary

At this point you have:

1. **120Hz main loop** — smooth animation ticks, scanner throttled separately
2. **Expression evaluator** — full math/comparison/logical expressions against sensor data
3. **Reactive class binding** — `class:name="expression"` adds/removes classes based on sensor conditions
4. **CSS transitions** — `transition: property duration easing` with smooth interpolation
5. **Easing functions** — linear, ease, ease-in, ease-out, ease-in-out, cubic-bezier
6. **Animatable properties** — opacity, width/height, colors, border-radius, padding/margin/gap, position
7. **Value interpolation** — numeric lerp, per-channel color lerp
8. **Transition state management** — per-element per-property active transitions with completion detection

**Next:** Phase 9a-4 adds structured error reporting for Monaco integration.
