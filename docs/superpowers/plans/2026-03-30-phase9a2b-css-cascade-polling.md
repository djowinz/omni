# Phase 9a-2b: CSS Cascade + Per-Sensor Polling

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **IMPORTANT: Run superpowers:code-reviewer after EVERY subagent task. No exceptions.**

**Goal:** Replace the hand-written CSS parser with `lightningcss` for full selector/specificity support, flatten the HTML tree for efficient ancestor matching, and refactor the sensor poller for per-sensor configurable poll intervals.

**Architecture:** Two independent subsystems. (1) CSS: `lightningcss` parses stylesheets, a new `FlatNode` tree representation enables descendant selector matching, specificity determines rule priority. (2) Polling: a `<config>` block in `.omni` files specifies per-sensor intervals, the poller thread runs at the fastest configured interval and polls each sensor group only when its timer elapses.

**Tech Stack:** Rust, `lightningcss` 1.0.0-alpha.71 (CSS parsing), `quick-xml` (config block parsing).

**Testing notes:** CSS matching is fully unit-testable with synthetic flat trees. Specificity ordering testable with known rules. Poll scheduling testable with mock timers. Full pipeline tested manually in-game.

**Depends on:** Phase 9a-2a complete (workspace, file management, .omni parser, resolver).

---

## File Map

```
host/
  src/
    omni/
      types.rs                   # Add PollConfig to OmniFile
      parser.rs                  # Add <config> block parsing
      css.rs                     # REWRITE: lightningcss integration + specificity
      resolver.rs                # Refactor: flatten tree, use new CSS matching
      flat_tree.rs               # NEW: FlatNode tree representation + ancestry
    sensors/
      mod.rs                     # Refactor: variable-rate polling with per-group timers
```

---

### Task 1: FlatNode Tree Representation

**Files:**
- Create: `host/src/omni/flat_tree.rs`
- Modify: `host/src/omni/mod.rs` (add `pub mod flat_tree;`)

This module flattens an `HtmlNode` tree into a `Vec<FlatNode>` with parent indices for O(1) ancestor lookups during CSS selector matching.

- [ ] **Step 1: Create host/src/omni/flat_tree.rs**

```rust
//! Flat tree representation for CSS selector matching.
//!
//! Converts an HtmlNode tree into a Vec<FlatNode> where each node
//! has a parent index for O(1) ancestor lookups. This enables efficient
//! descendant selector matching regardless of tree depth.

use super::types::HtmlNode;

/// A flattened node with parent reference for ancestor traversal.
#[derive(Debug, Clone)]
pub struct FlatNode {
    /// The tag name (e.g., "div", "span"). Empty for text nodes.
    pub tag: String,
    /// Element ID attribute.
    pub id: Option<String>,
    /// CSS classes.
    pub classes: Vec<String>,
    /// Inline style attribute (unparsed).
    pub inline_style: Option<String>,
    /// Index of parent in the flat list. None for root.
    pub parent_index: Option<usize>,
    /// Nesting depth (0 for root).
    pub depth: usize,
    /// True if this is a text node.
    pub is_text: bool,
    /// Text content (for text nodes only).
    pub text_content: Option<String>,
    /// Indices of child nodes in the flat list.
    pub child_indices: Vec<usize>,
}

/// Flatten an HtmlNode tree into a Vec<FlatNode>.
pub fn flatten_tree(root: &HtmlNode) -> Vec<FlatNode> {
    let mut nodes = Vec::new();
    flatten_recursive(root, None, 0, &mut nodes);
    nodes
}

fn flatten_recursive(
    node: &HtmlNode,
    parent_index: Option<usize>,
    depth: usize,
    nodes: &mut Vec<FlatNode>,
) {
    let my_index = nodes.len();

    match node {
        HtmlNode::Element { tag, id, classes, inline_style, children } => {
            nodes.push(FlatNode {
                tag: tag.clone(),
                id: id.clone(),
                classes: classes.clone(),
                inline_style: inline_style.clone(),
                parent_index,
                depth,
                is_text: false,
                text_content: None,
                child_indices: Vec::new(), // filled after children are added
            });

            // Record parent's child index
            if let Some(pi) = parent_index {
                nodes[pi].child_indices.push(my_index);
            }

            // Flatten children
            let mut child_indices = Vec::new();
            for child in children {
                let child_idx = nodes.len();
                flatten_recursive(child, Some(my_index), depth + 1, nodes);
                child_indices.push(child_idx);
            }

            // Update this node's child_indices
            nodes[my_index].child_indices = child_indices;
        }
        HtmlNode::Text { content } => {
            nodes.push(FlatNode {
                tag: String::new(),
                id: None,
                classes: Vec::new(),
                inline_style: None,
                parent_index,
                depth,
                is_text: true,
                text_content: Some(content.clone()),
                child_indices: Vec::new(),
            });

            if let Some(pi) = parent_index {
                nodes[pi].child_indices.push(my_index);
            }
        }
    }
}

/// Get the ancestor chain for a node (from immediate parent up to root).
/// Returns a list of indices from parent to root.
pub fn ancestor_chain(nodes: &[FlatNode], index: usize) -> Vec<usize> {
    let mut chain = Vec::new();
    let mut current = nodes[index].parent_index;
    while let Some(idx) = current {
        chain.push(idx);
        current = nodes[idx].parent_index;
    }
    chain
}

/// Check if any ancestor of the node at `index` has the given class.
pub fn has_ancestor_with_class(nodes: &[FlatNode], index: usize, class: &str) -> bool {
    let mut current = nodes[index].parent_index;
    while let Some(idx) = current {
        if nodes[idx].classes.iter().any(|c| c == class) {
            return true;
        }
        current = nodes[idx].parent_index;
    }
    false
}

/// Check if any ancestor of the node at `index` has the given ID.
pub fn has_ancestor_with_id(nodes: &[FlatNode], index: usize, id: &str) -> bool {
    let mut current = nodes[index].parent_index;
    while let Some(idx) = current {
        if nodes[idx].id.as_deref() == Some(id) {
            return true;
        }
        current = nodes[idx].parent_index;
    }
    false
}

/// Check if any ancestor of the node at `index` has the given tag.
pub fn has_ancestor_with_tag(nodes: &[FlatNode], index: usize, tag: &str) -> bool {
    let mut current = nodes[index].parent_index;
    while let Some(idx) = current {
        if nodes[idx].tag == tag {
            return true;
        }
        current = nodes[idx].parent_index;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree() -> HtmlNode {
        // <div class="panel">
        //   <div class="row">
        //     <span class="value critical" id="cpu">text</span>
        //   </div>
        //   <span class="label">label</span>
        // </div>
        HtmlNode::Element {
            tag: "div".to_string(),
            id: None,
            classes: vec!["panel".to_string()],
            inline_style: None,
            children: vec![
                HtmlNode::Element {
                    tag: "div".to_string(),
                    id: None,
                    classes: vec!["row".to_string()],
                    inline_style: None,
                    children: vec![
                        HtmlNode::Element {
                            tag: "span".to_string(),
                            id: Some("cpu".to_string()),
                            classes: vec!["value".to_string(), "critical".to_string()],
                            inline_style: Some("color: red;".to_string()),
                            children: vec![
                                HtmlNode::Text { content: "text".to_string() },
                            ],
                        },
                    ],
                },
                HtmlNode::Element {
                    tag: "span".to_string(),
                    id: None,
                    classes: vec!["label".to_string()],
                    inline_style: None,
                    children: vec![
                        HtmlNode::Text { content: "label".to_string() },
                    ],
                },
            ],
        }
    }

    #[test]
    fn flatten_produces_correct_count() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);
        // div.panel > div.row > span#cpu > "text"
        //           > span.label > "label"
        // = 6 nodes total
        assert_eq!(flat.len(), 6);
    }

    #[test]
    fn parent_indices_correct() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);

        // Root div.panel at index 0 — no parent
        assert_eq!(flat[0].parent_index, None);
        assert_eq!(flat[0].tag, "div");

        // div.row at index 1 — parent is 0
        assert_eq!(flat[1].parent_index, Some(0));

        // span#cpu at index 2 — parent is 1 (div.row)
        assert_eq!(flat[2].parent_index, Some(1));
        assert_eq!(flat[2].id.as_deref(), Some("cpu"));

        // "text" at index 3 — parent is 2 (span#cpu)
        assert!(flat[3].is_text);
        assert_eq!(flat[3].parent_index, Some(2));
    }

    #[test]
    fn ancestor_chain_correct() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);

        // span#cpu (index 2) ancestors: div.row (1), div.panel (0)
        let chain = ancestor_chain(&flat, 2);
        assert_eq!(chain, vec![1, 0]);
    }

    #[test]
    fn ancestor_class_check() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);

        // span#cpu (index 2) has ancestor with class "panel"
        assert!(has_ancestor_with_class(&flat, 2, "panel"));
        assert!(has_ancestor_with_class(&flat, 2, "row"));
        assert!(!has_ancestor_with_class(&flat, 2, "nonexistent"));
    }

    #[test]
    fn deeply_nested_tree() {
        // Create a 20-level deep tree
        let mut node = HtmlNode::Text { content: "deep".to_string() };
        for i in 0..20 {
            node = HtmlNode::Element {
                tag: "div".to_string(),
                id: None,
                classes: vec![format!("level-{}", i)],
                inline_style: None,
                children: vec![node],
            };
        }

        let flat = flatten_tree(&node);
        assert_eq!(flat.len(), 21); // 20 divs + 1 text

        // Deepest node should have ancestor chain of length 20
        let chain = ancestor_chain(&flat, 20);
        assert_eq!(chain.len(), 20);
    }

    #[test]
    fn child_indices_correct() {
        let tree = make_tree();
        let flat = flatten_tree(&tree);

        // Root div.panel has 2 element children: div.row (1) and span.label (4)
        assert_eq!(flat[0].child_indices.len(), 2);
        assert_eq!(flat[0].child_indices[0], 1); // div.row
        assert_eq!(flat[0].child_indices[1], 4); // span.label
    }
}
```

- [ ] **Step 2: Add module to mod.rs**

Add `pub mod flat_tree;` to `host/src/omni/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host -- omni::flat_tree`
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/flat_tree.rs host/src/omni/mod.rs
git commit -m "feat(host): add FlatNode tree representation for CSS ancestor matching"
```

---

### Task 2: Rewrite CSS Parser with lightningcss

**Files:**
- Rewrite: `host/src/omni/css.rs`

Replace the hand-written CSS parser with `lightningcss`. This gives us full selector parsing, specificity calculation, and proper CSS value handling.

- [ ] **Step 1: Rewrite host/src/omni/css.rs**

The new module needs to:
1. Parse CSS source with `lightningcss::stylesheet::StyleSheet::parse`
2. Extract `:root` variables
3. For a given flat node, find all matching rules and compute specificity
4. Merge properties in specificity order (lowest first, highest wins)
5. Apply inline styles last (highest priority)
6. Resolve `var()` references

IMPORTANT: `lightningcss` 1.0.0-alpha.71 has a specific API. The implementer MUST:
- Check the actual crate API (it may differ from stable lightningcss)
- Use `StyleSheet::parse` with appropriate `ParserOptions`
- Iterate `stylesheet.rules` to extract `CssRule::Style` variants
- Use the `Selector` type from lightningcss for matching
- Extract specificity from the selector

If `lightningcss`'s selector matching API is too complex to integrate directly with our `FlatNode` tree, an alternative approach is acceptable:
- Use `lightningcss` ONLY for parsing CSS text into structured rules (selectors + properties)
- Write our own selector matching against `FlatNode` (simpler than parsing CSS)
- Use `lightningcss`'s specificity calculation

The key types to produce:

```rust
/// A parsed CSS rule with specificity.
pub struct CssRule {
    pub selector: ParsedSelector,
    pub properties: HashMap<String, String>,
    pub specificity: (u32, u32, u32), // (ids, classes, elements)
}

/// A parsed selector that can be matched against FlatNodes.
pub enum ParsedSelector {
    Simple(SimpleSelector),
    Descendant(Vec<SimpleSelector>), // right-to-left: [target, ancestor1, ancestor2...]
    Root,
}

pub struct SimpleSelector {
    pub element: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
}
```

`resolve_styles` function signature changes to accept `FlatNode` context:

```rust
pub fn resolve_styles(
    node: &FlatNode,
    node_index: usize,
    flat_tree: &[FlatNode],
    stylesheet: &ParsedStylesheet,
    theme_vars: &HashMap<String, String>,
) -> ResolvedStyle
```

Existing tests must be updated or replaced. New tests needed for:
- Descendant selector matching (`.panel .label` matches span inside div.panel)
- Compound selector matching (`.value.critical` matches element with both classes)
- Specificity ordering (#id > .class > element)
- Specificity: more specific rule wins regardless of source order
- `var()` resolution still works
- Theme variables still apply

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Compiles. The resolver will need updating (Task 3) so it may have temporary errors if it still calls the old API.

- [ ] **Step 3: Run CSS tests**

Run: `cargo test -p omni-host -- omni::css`
Expected: All new tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/css.rs
git commit -m "feat(host): rewrite CSS parser with lightningcss + specificity + descendant selectors"
```

---

### Task 3: Update Resolver to Use Flat Tree + New CSS

**Files:**
- Modify: `host/src/omni/resolver.rs`

Refactor the resolver to:
1. Flatten each widget's template tree before resolution
2. Iterate flat nodes instead of recursive tree walk
3. Call the new `css::resolve_styles` with flat node context
4. Produce the same `Vec<ComputedWidget>` output

- [ ] **Step 1: Rewrite the resolve method**

The new flow for each enabled widget:
1. `flat_tree::flatten_tree(&widget.template)` → `Vec<FlatNode>`
2. `css::parse_css(&widget.style_source)` → `ParsedStylesheet`
3. For each non-text `FlatNode`:
   - `css::resolve_styles(node, index, &flat_nodes, &stylesheet, &theme_vars)` → `ResolvedStyle`
   - Interpolate inline styles with sensor data
   - Check if node has text children (collect from child_indices where is_text)
   - Emit `ComputedWidget` with resolved position, colors, text

The position computation needs updating since we're iterating flat nodes, not recursively walking. Use `parent_index` to look up the parent's resolved position for relative positioning.

IMPORTANT: The `label_text` field must still be set with the raw template text for DLL frame timing interpolation.

- [ ] **Step 2: Update resolver tests**

Existing tests (resolve_simple_widget, disabled_widget, theme_variables, parse_color_formats) should still pass with the same behavior.

Add new tests:
- Descendant selector styles apply correctly
- Compound selector styles apply correctly
- Higher specificity rule wins

- [ ] **Step 3: Verify all tests pass**

Run: `cargo test -p omni-host -- omni`
Expected: All omni tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/resolver.rs
git commit -m "feat(host): refactor resolver to use flat tree + lightningcss CSS matching"
```

---

### Task 4: Add `<config>` Block Parsing

**Files:**
- Modify: `host/src/omni/types.rs`
- Modify: `host/src/omni/parser.rs`

- [ ] **Step 1: Add PollConfig to OmniFile**

In `types.rs`, add:

```rust
use std::collections::HashMap;

/// A parsed .omni file containing a theme reference and widget definitions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OmniFile {
    /// Optional path to a theme CSS file.
    pub theme_src: Option<String>,
    /// Per-sensor poll interval configuration.
    pub poll_config: HashMap<String, u64>,
    /// Ordered list of widget definitions.
    pub widgets: Vec<Widget>,
}
```

Update `OmniFile::empty()`:

```rust
pub fn empty() -> Self {
    Self {
        theme_src: None,
        poll_config: HashMap::new(),
        widgets: Vec::new(),
    }
}
```

- [ ] **Step 2: Parse `<config>` block in parser.rs**

In the top-level parsing loop (where `<widget>` and `<theme>` are handled), add a case for `<config>`:

```rust
b"config" => {
    poll_config = parse_config_block(&mut reader)?;
}
```

The `parse_config_block` function reads `<poll sensor="..." interval="..." />` elements:

```rust
fn parse_config_block(reader: &mut Reader<&[u8]>) -> Result<HashMap<String, u64>, ParseError> {
    let mut config = HashMap::new();

    loop {
        match reader.read_event() {
            Ok(Event::Empty(ref e)) if e.name().as_ref() == b"poll" => {
                let sensor = get_attr(e, "sensor");
                let interval = get_attr(e, "interval")
                    .and_then(|v| v.parse::<u64>().ok());

                if let (Some(sensor), Some(interval)) = (sensor, interval) {
                    config.insert(sensor, interval);
                }
            }
            Ok(Event::End(ref e)) if e.name().as_ref() == b"config" => break,
            Ok(Event::Eof) => {
                return Err(ParseError {
                    message: "Unexpected EOF inside <config>".to_string(),
                    offset: reader.buffer_position() as usize,
                });
            }
            _ => {}
        }
    }

    Ok(config)
}
```

- [ ] **Step 3: Add tests**

```rust
#[test]
fn parse_config_block() {
    let source = r#"
        <config>
            <poll sensor="fps" interval="100" />
            <poll sensor="gpu.temp" interval="250" />
            <poll sensor="cpu.usage" interval="1000" />
        </config>
        <widget id="test" name="Test" enabled="true">
            <template><span>test</span></template>
            <style></style>
        </widget>
    "#;

    let file = parse_omni(source).unwrap();
    assert_eq!(file.poll_config.len(), 3);
    assert_eq!(file.poll_config.get("fps"), Some(&100));
    assert_eq!(file.poll_config.get("gpu.temp"), Some(&250));
    assert_eq!(file.poll_config.get("cpu.usage"), Some(&1000));
}

#[test]
fn omni_file_without_config_has_empty_poll_config() {
    let source = r#"
        <widget id="test" name="Test" enabled="true">
            <template><span>test</span></template>
            <style></style>
        </widget>
    "#;

    let file = parse_omni(source).unwrap();
    assert!(file.poll_config.is_empty());
}
```

- [ ] **Step 4: Verify all tests pass**

Run: `cargo test -p omni-host -- omni`
Expected: All tests pass including new config tests.

- [ ] **Step 5: Commit**

```bash
git add host/src/omni/types.rs host/src/omni/parser.rs
git commit -m "feat(host): parse <config> block with per-sensor poll intervals"
```

---

### Task 5: Refactor Sensor Poller for Variable-Rate Polling

**Files:**
- Modify: `host/src/sensors/mod.rs`

- [ ] **Step 1: Rewrite SensorPoller with per-group timers**

The poller needs to:
1. Accept a `HashMap<String, u64>` poll config
2. Group sensors: CPU group (cpu.usage, cpu.temp), GPU group (gpu.*), RAM group (ram.*)
3. Compute base tick = GCD of all intervals (minimum 50ms)
4. Each tick, check which groups are due and only poll those
5. Maintain a running `SensorSnapshot` that's updated incrementally

```rust
use std::collections::HashMap;
use std::time::Instant;

/// Default poll interval for sensors not explicitly configured.
const DEFAULT_POLL_MS: u64 = 1000;
/// Minimum base tick to avoid busy-spinning.
const MIN_BASE_TICK_MS: u64 = 50;

/// Sensor groups that poll together.
struct SensorGroup {
    interval_ms: u64,
    last_poll: Instant,
}

impl SensorPoller {
    pub fn start(
        poll_config: HashMap<String, u64>,
        running: Arc<AtomicBool>,
    ) -> (Self, mpsc::Receiver<SensorSnapshot>) {
        let (tx, rx) = mpsc::channel();
        let running_clone = running.clone();

        let handle = thread::spawn(move || {
            let mut system = System::new();
            system.refresh_cpu_all();
            system.refresh_memory();

            let cpu = CpuPoller::new(&system);
            let cpu_temp = CpuTempPoller::new();
            let gpu = GpuPoller::new();
            let ram = RamPoller::new();

            // Determine per-group intervals
            let cpu_interval = *["cpu.usage", "cpu.temp"].iter()
                .filter_map(|k| poll_config.get(*k))
                .min()
                .unwrap_or(&DEFAULT_POLL_MS);

            let gpu_interval = *["gpu.usage", "gpu.temp", "gpu.clock", "gpu.mem-clock",
                                  "gpu.vram", "gpu.power", "gpu.fan"].iter()
                .filter_map(|k| poll_config.get(*k))
                .min()
                .unwrap_or(&DEFAULT_POLL_MS);

            let ram_interval = *["ram.usage"].iter()
                .filter_map(|k| poll_config.get(*k))
                .min()
                .unwrap_or(&DEFAULT_POLL_MS);

            let base_tick = gcd(gcd(cpu_interval, gpu_interval), ram_interval)
                .max(MIN_BASE_TICK_MS);

            info!(
                cpu_ms = cpu_interval,
                gpu_ms = gpu_interval,
                ram_ms = ram_interval,
                base_tick_ms = base_tick,
                "Sensor polling configured"
            );

            let now = Instant::now();
            let mut cpu_group = SensorGroup { interval_ms: cpu_interval, last_poll: now };
            let mut gpu_group = SensorGroup { interval_ms: gpu_interval, last_poll: now };
            let mut ram_group = SensorGroup { interval_ms: ram_interval, last_poll: now };

            // sysinfo needs two samples
            thread::sleep(Duration::from_millis(500));
            info!("Sensor polling started");

            let mut snapshot = SensorSnapshot::default();

            while running_clone.load(Ordering::Relaxed) {
                let now = Instant::now();
                let mut any_updated = false;

                // CPU group
                if now.duration_since(cpu_group.last_poll).as_millis() >= cpu_group.interval_ms as u128 {
                    system.refresh_cpu_all();
                    let mut cpu_data = cpu.poll(&system);
                    cpu_data.package_temp_c = cpu_temp.poll();
                    snapshot.cpu = cpu_data;
                    cpu_group.last_poll = now;
                    any_updated = true;
                }

                // GPU group
                if now.duration_since(gpu_group.last_poll).as_millis() >= gpu_group.interval_ms as u128 {
                    snapshot.gpu = match &gpu {
                        Some(g) => g.poll(),
                        None => omni_shared::GpuData::default(),
                    };
                    gpu_group.last_poll = now;
                    any_updated = true;
                }

                // RAM group
                if now.duration_since(ram_group.last_poll).as_millis() >= ram_group.interval_ms as u128 {
                    system.refresh_memory();
                    snapshot.ram = ram.poll(&system);
                    ram_group.last_poll = now;
                    any_updated = true;
                }

                if any_updated {
                    snapshot.timestamp_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;

                    if tx.send(snapshot).is_err() {
                        break;
                    }
                }

                thread::sleep(Duration::from_millis(base_tick));
            }
        });

        (Self { handle: Some(handle), running }, rx)
    }
}

/// Greatest common divisor.
fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}
```

- [ ] **Step 2: Update main.rs to pass poll_config**

In `run_host`, extract `poll_config` from the parsed `omni_file` and pass it to `SensorPoller::start`:

Replace:
```rust
let (mut sensor_poller, sensor_rx) = sensors::SensorPoller::start(
    Duration::from_millis(1000),
    sensor_running,
);
```

With:
```rust
let (mut sensor_poller, sensor_rx) = sensors::SensorPoller::start(
    omni_file.poll_config.clone(),
    sensor_running,
);
```

- [ ] **Step 3: Add tests**

```rust
#[test]
fn gcd_computation() {
    assert_eq!(gcd(1000, 250), 250);
    assert_eq!(gcd(100, 1000), 100);
    assert_eq!(gcd(300, 200), 100);
    assert_eq!(gcd(0, 500), 500);
}
```

- [ ] **Step 4: Verify all tests pass**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add host/src/sensors/mod.rs host/src/main.rs
git commit -m "feat(host): refactor sensor poller for per-group configurable poll intervals"
```

---

### Task 6: Integration Test — CSS Cascade + Per-Sensor Polling

This is a manual integration test.

- [ ] **Step 1: Build everything**

```bash
cargo build -p omni-host && cargo build -p omni-overlay-dll
```

- [ ] **Step 2: Test descendant selectors**

Create an overlay at `%APPDATA%\Omni\overlays\Default\overlay.omni`:

```xml
<widget id="test" name="Test" enabled="true">
  <template>
    <div class="panel" style="position: fixed; top: 10px; left: 10px;">
      <div class="section">
        <span class="value">CPU: {cpu.usage}%</span>
        <span class="value critical">GPU: {gpu.temp}°C</span>
      </div>
    </div>
  </template>
  <style>
    .panel { background: rgba(20,20,20,0.7); border-radius: 8px; padding: 10px; display: flex; flex-direction: column; gap: 4px; }
    .value { color: white; font-size: 14px; }
    .panel .critical { color: #ff4444; }
    #test { font-weight: bold; }
  </style>
</widget>
```

Verify: "GPU: XX°C" text is red (#ff4444) due to the descendant selector `.panel .critical`. Other `.value` text is white.

- [ ] **Step 3: Test per-sensor polling**

Add a config block to the overlay:

```xml
<config>
  <poll sensor="gpu.temp" interval="250" />
  <poll sensor="cpu.usage" interval="2000" />
</config>
```

Restart the host. Check logs for:
```
INFO Sensor polling configured cpu_ms=2000 gpu_ms=250 ram_ms=1000 base_tick_ms=250
```

GPU temp should update ~4x/sec while CPU updates ~0.5x/sec.

- [ ] **Step 4: Verify resilience**

- Ctrl+C → restart → overlay works with custom CSS
- Kill via Task Manager → restart → reconnects
- widget.apply via WebSocket → CSS selectors work in live preview

- [ ] **Step 5: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 9a-2b integration test"
```

---

## Phase 9a-2b Complete — Summary

At this point you have:

1. **FlatNode tree** — HTML tree flattened with parent indices for O(1) ancestor lookups
2. **lightningcss integration** — full CSS parsing replacing hand-written parser
3. **Descendant selectors** — `.panel .label` matches based on ancestry
4. **Compound selectors** — `.label.critical` matches elements with multiple classes
5. **Specificity** — ID > class > element, higher specificity wins regardless of source order
6. **`<config>` block** — per-sensor poll intervals in `.omni` files
7. **Variable-rate polling** — sensor groups poll independently at configured rates

**Next:** Phase 9a-3 adds advanced visuals (gradients, box-shadow) and taffy flexbox layout.
