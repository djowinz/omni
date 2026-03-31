//! Taffy-based flexbox layout engine.
//!
//! Takes flat nodes, resolved styles, and text measurements, builds a taffy
//! tree, computes layout, and returns absolute positions and sizes.

use super::flat_tree::FlatNode;
use super::types::ResolvedStyle;
use taffy::prelude::*;
use taffy::style::{
    AlignItems, Dimension, Display, FlexDirection, FlexWrap, LengthPercentage,
    LengthPercentageAuto, Position,
};
/// The computed absolute position and size for a single node.
#[derive(Debug, Clone, Default)]
pub struct LayoutResult {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Compute absolute layout positions for all flat nodes using taffy flexbox.
///
/// # Arguments
/// - `flat_nodes` - The flattened DOM tree
/// - `styles` - Resolved CSS styles per node (indices match flat_nodes)
/// - `text_sizes` - Measured (width, height) per node; (0,0) for non-text
/// - `available_width` - Viewport width
/// - `available_height` - Viewport height
///
/// # Returns
/// A `Vec<LayoutResult>` with absolute positions for each node.
pub fn compute_layout(
    flat_nodes: &[FlatNode],
    styles: &[ResolvedStyle],
    text_sizes: &[(f32, f32)],
    available_width: f32,
    available_height: f32,
) -> Vec<LayoutResult> {
    let n = flat_nodes.len();
    if n == 0 {
        return Vec::new();
    }

    let mut tree: TaffyTree<()> = TaffyTree::new();

    // Phase 1: Create a taffy node for every flat node (as leaves initially).
    // Text nodes inherit their parent's measured text size so taffy knows
    // how much space the text content needs.
    let mut taffy_ids: Vec<NodeId> = Vec::with_capacity(n);
    for i in 0..n {
        let effective_text_size = if flat_nodes[i].is_text {
            // Text node: use parent's measured text size
            flat_nodes[i].parent_index
                .map(|pi| text_sizes[pi])
                .unwrap_or((0.0, 0.0))
        } else {
            text_sizes[i]
        };
        let style = build_taffy_style(&styles[i], &flat_nodes[i], effective_text_size);
        let node_id = tree.new_leaf(style).expect("taffy new_leaf failed");
        taffy_ids.push(node_id);
    }

    // Phase 2: Wire up parent-child relationships.
    // Process in reverse so that when we set children for a parent, all
    // children already exist. We use set_children which replaces any existing
    // children list.
    for i in 0..n {
        let child_taffy_ids: Vec<NodeId> = flat_nodes[i]
            .child_indices
            .iter()
            .map(|&ci| taffy_ids[ci])
            .collect();
        if !child_taffy_ids.is_empty() {
            tree.set_children(taffy_ids[i], &child_taffy_ids)
                .expect("taffy set_children failed");
        }
    }

    // Phase 3: Find root node(s). Nodes without a parent are roots.
    // Taffy needs a single root, so if there are multiple roots we wrap them.
    let root_indices: Vec<usize> = (0..n)
        .filter(|&i| flat_nodes[i].parent_index.is_none())
        .collect();

    // ALWAYS wrap roots in a viewport container.
    // This is required for position:absolute/fixed to work — taffy positions
    // absolute elements relative to their containing block (parent), so without
    // a viewport wrapper, absolute positioning has no reference frame.
    let wrapper_children: Vec<NodeId> = root_indices.iter().map(|&i| taffy_ids[i]).collect();
    let layout_root = tree.new_with_children(
        Style {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            size: Size {
                width: Dimension::Length(available_width),
                height: Dimension::Length(available_height),
            },
            ..Style::DEFAULT
        },
        &wrapper_children,
    )
    .expect("taffy viewport wrapper node failed");

    // Phase 4: Compute layout.
    tree.compute_layout(
        layout_root,
        Size {
            width: AvailableSpace::Definite(available_width),
            height: AvailableSpace::Definite(available_height),
        },
    )
    .expect("taffy compute_layout failed");

    // Phase 5: Extract results and convert relative positions to absolute.
    let mut results = vec![LayoutResult::default(); n];

    // Compute absolute positions by walking from roots down.
    // For each node, absolute = parent_absolute + node.location
    for i in 0..n {
        let layout = tree
            .layout(taffy_ids[i])
            .expect("taffy layout query failed");

        let rel_x = layout.location.x;
        let rel_y = layout.location.y;

        let (parent_abs_x, parent_abs_y) = match flat_nodes[i].parent_index {
            Some(pi) => (results[pi].x, results[pi].y),
            None => {
                // Root nodes: parent is the viewport wrapper (always present)
                let wrapper_layout = tree
                    .layout(layout_root)
                    .expect("taffy viewport layout query failed");
                (wrapper_layout.location.x, wrapper_layout.location.y)
            }
        };

        results[i] = LayoutResult {
            x: parent_abs_x + rel_x,
            y: parent_abs_y + rel_y,
            width: layout.size.width,
            height: layout.size.height,
        };
    }

    results
}

/// Parse a CSS pixel value like "12px" or "14" into f32.
fn parse_px(val: Option<&str>) -> Option<f32> {
    let s = val?.trim();
    if s == "auto" || s == "none" || s.is_empty() {
        return None;
    }
    let s = s.trim_end_matches("px");
    s.parse::<f32>().ok()
}

/// Build a taffy `Style` from a `ResolvedStyle` + flat node info + text size.
fn build_taffy_style(
    style: &ResolvedStyle,
    node: &FlatNode,
    text_size: (f32, f32),
) -> Style {
    let mut ts = Style {
        display: Display::Flex,
        ..Style::DEFAULT
    };

    // Display
    if let Some(d) = &style.display {
        ts.display = match d.as_str() {
            "flex" => Display::Flex,
            "none" => Display::None,
            _ => Display::Flex,
        };
    }

    // Position
    if let Some(pos) = &style.position {
        match pos.as_str() {
            "fixed" | "absolute" => {
                ts.position = Position::Absolute;
                // Set inset from top/left/right/bottom
                ts.inset = Rect {
                    top: parse_lpa(style.top.as_deref()),
                    right: parse_lpa(style.right.as_deref()),
                    bottom: parse_lpa(style.bottom.as_deref()),
                    left: parse_lpa(style.left.as_deref()),
                };
            }
            _ => {}
        }
    }

    // Flex direction
    if let Some(fd) = &style.flex_direction {
        ts.flex_direction = match fd.as_str() {
            "column" => FlexDirection::Column,
            "row" => FlexDirection::Row,
            "column-reverse" => FlexDirection::ColumnReverse,
            "row-reverse" => FlexDirection::RowReverse,
            _ => FlexDirection::Row,
        };
    }

    // Justify content
    if let Some(jc) = &style.justify_content {
        ts.justify_content = match jc.as_str() {
            "center" => Some(JustifyContent::Center),
            "flex-start" | "start" => Some(JustifyContent::FlexStart),
            "flex-end" | "end" => Some(JustifyContent::FlexEnd),
            "space-between" => Some(JustifyContent::SpaceBetween),
            "space-around" => Some(JustifyContent::SpaceAround),
            "space-evenly" => Some(JustifyContent::SpaceEvenly),
            "stretch" => Some(JustifyContent::Stretch),
            _ => None,
        };
    }

    // Align items
    if let Some(ai) = &style.align_items {
        ts.align_items = match ai.as_str() {
            "center" => Some(AlignItems::Center),
            "flex-start" | "start" => Some(AlignItems::FlexStart),
            "flex-end" | "end" => Some(AlignItems::FlexEnd),
            "stretch" => Some(AlignItems::Stretch),
            "baseline" => Some(AlignItems::Baseline),
            _ => None,
        };
    }

    // Align self
    if let Some(als) = &style.align_self {
        ts.align_self = match als.as_str() {
            "center" => Some(AlignItems::Center),
            "flex-start" | "start" => Some(AlignItems::FlexStart),
            "flex-end" | "end" => Some(AlignItems::FlexEnd),
            "stretch" => Some(AlignItems::Stretch),
            "baseline" => Some(AlignItems::Baseline),
            _ => None,
        };
    }

    // Flex grow / shrink
    if let Some(fg) = &style.flex_grow {
        if let Ok(v) = fg.parse::<f32>() {
            ts.flex_grow = v;
        }
    }
    if let Some(fs) = &style.flex_shrink {
        if let Ok(v) = fs.parse::<f32>() {
            ts.flex_shrink = v;
        }
    }

    // Flex wrap
    if let Some(fw) = &style.flex_wrap {
        ts.flex_wrap = match fw.as_str() {
            "wrap" => FlexWrap::Wrap,
            "wrap-reverse" => FlexWrap::WrapReverse,
            _ => FlexWrap::NoWrap,
        };
    }

    // Gap
    if let Some(gap_str) = &style.gap {
        if let Some(g) = parse_px(Some(gap_str)) {
            ts.gap = Size {
                width: LengthPercentage::Length(g),
                height: LengthPercentage::Length(g),
            };
        }
    }

    // Padding
    if let Some(p) = &style.padding {
        if let Some(v) = parse_px(Some(p)) {
            ts.padding = Rect {
                top: LengthPercentage::Length(v),
                right: LengthPercentage::Length(v),
                bottom: LengthPercentage::Length(v),
                left: LengthPercentage::Length(v),
            };
        }
    }

    // Margin
    if let Some(m) = &style.margin {
        if let Some(v) = parse_px(Some(m)) {
            ts.margin = Rect {
                top: LengthPercentageAuto::Length(v),
                right: LengthPercentageAuto::Length(v),
                bottom: LengthPercentageAuto::Length(v),
                left: LengthPercentageAuto::Length(v),
            };
        }
    }

    // Size (width / height)
    ts.size = Size {
        width: parse_dimension(style.width.as_deref()),
        height: parse_dimension(style.height.as_deref()),
    };

    // Min/max size
    ts.min_size = Size {
        width: parse_dimension(style.min_width.as_deref()),
        height: parse_dimension(style.min_height.as_deref()),
    };
    ts.max_size = Size {
        width: parse_dimension(style.max_width.as_deref()),
        height: parse_dimension(style.max_height.as_deref()),
    };

    // Text nodes: set fixed size from measured dimensions
    if node.is_text && (text_size.0 > 0.0 || text_size.1 > 0.0) {
        ts.size = Size {
            width: Dimension::Length(text_size.0),
            height: Dimension::Length(text_size.1),
        };
    }

    // Non-text nodes with text children: if no explicit size, use text_size as min
    if !node.is_text && (text_size.0 > 0.0 || text_size.1 > 0.0) {
        if ts.size.width == Dimension::Auto && ts.min_size.width == Dimension::Auto {
            ts.min_size.width = Dimension::Length(text_size.0);
        }
        if ts.size.height == Dimension::Auto && ts.min_size.height == Dimension::Auto {
            ts.min_size.height = Dimension::Length(text_size.1);
        }
    }

    ts
}

/// Parse a CSS value into a `Dimension`.
fn parse_dimension(val: Option<&str>) -> Dimension {
    match val {
        None => Dimension::Auto,
        Some(s) => {
            let s = s.trim();
            if s == "auto" || s.is_empty() {
                Dimension::Auto
            } else if s.ends_with('%') {
                s.trim_end_matches('%')
                    .parse::<f32>()
                    .map(|v| Dimension::Percent(v / 100.0))
                    .unwrap_or(Dimension::Auto)
            } else {
                parse_px(Some(s))
                    .map(Dimension::Length)
                    .unwrap_or(Dimension::Auto)
            }
        }
    }
}

/// Parse a CSS value into `LengthPercentageAuto`.
fn parse_lpa(val: Option<&str>) -> LengthPercentageAuto {
    match val {
        None => LengthPercentageAuto::Auto,
        Some(s) => {
            let s = s.trim();
            if s == "auto" || s.is_empty() {
                LengthPercentageAuto::Auto
            } else if s.ends_with('%') {
                s.trim_end_matches('%')
                    .parse::<f32>()
                    .map(|v| LengthPercentageAuto::Percent(v / 100.0))
                    .unwrap_or(LengthPercentageAuto::Auto)
            } else {
                parse_px(Some(s))
                    .map(LengthPercentageAuto::Length)
                    .unwrap_or(LengthPercentageAuto::Auto)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a non-text FlatNode element.
    fn elem(
        tag: &str,
        parent_index: Option<usize>,
        child_indices: Vec<usize>,
    ) -> FlatNode {
        FlatNode {
            tag: tag.to_string(),
            id: None,
            classes: Vec::new(),
            inline_style: None,
            conditional_classes: Vec::new(),
            parent_index,
            depth: 0,
            is_text: false,
            text_content: None,
            child_indices,
        }
    }

    /// Helper: create a text FlatNode.
    fn text_node(parent_index: Option<usize>) -> FlatNode {
        FlatNode {
            tag: String::new(),
            id: None,
            classes: Vec::new(),
            inline_style: None,
            conditional_classes: Vec::new(),
            parent_index,
            depth: 1,
            is_text: true,
            text_content: Some("hello".to_string()),
            child_indices: Vec::new(),
        }
    }

    /// Helper: default style with overrides.
    fn style_with(f: impl FnOnce(&mut ResolvedStyle)) -> ResolvedStyle {
        let mut s = ResolvedStyle::default();
        f(&mut s);
        s
    }

    #[test]
    fn vertical_column_layout() {
        // Root (column) with 3 children, each 100px x 30px
        let nodes = vec![
            elem("div", None, vec![1, 2, 3]),
            elem("div", Some(0), vec![]),
            elem("div", Some(0), vec![]),
            elem("div", Some(0), vec![]),
        ];
        let styles = vec![
            style_with(|s| {
                s.display = Some("flex".into());
                s.flex_direction = Some("column".into());
                s.width = Some("300px".into());
                s.height = Some("300px".into());
            }),
            style_with(|s| {
                s.width = Some("100px".into());
                s.height = Some("30px".into());
            }),
            style_with(|s| {
                s.width = Some("100px".into());
                s.height = Some("30px".into());
            }),
            style_with(|s| {
                s.width = Some("100px".into());
                s.height = Some("30px".into());
            }),
        ];
        let text_sizes = vec![(0.0, 0.0); 4];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        assert_eq!(results.len(), 4);
        // Children stack vertically
        assert_eq!(results[1].y, 0.0);
        assert_eq!(results[2].y, 30.0);
        assert_eq!(results[3].y, 60.0);
        // All children have correct dimensions
        assert_eq!(results[1].width, 100.0);
        assert_eq!(results[1].height, 30.0);
    }

    #[test]
    fn horizontal_row_layout() {
        // Root (row) with 3 children, each 50px x 40px
        let nodes = vec![
            elem("div", None, vec![1, 2, 3]),
            elem("div", Some(0), vec![]),
            elem("div", Some(0), vec![]),
            elem("div", Some(0), vec![]),
        ];
        let styles = vec![
            style_with(|s| {
                s.display = Some("flex".into());
                s.flex_direction = Some("row".into());
                s.width = Some("300px".into());
                s.height = Some("200px".into());
            }),
            style_with(|s| {
                s.width = Some("50px".into());
                s.height = Some("40px".into());
            }),
            style_with(|s| {
                s.width = Some("50px".into());
                s.height = Some("40px".into());
            }),
            style_with(|s| {
                s.width = Some("50px".into());
                s.height = Some("40px".into());
            }),
        ];
        let text_sizes = vec![(0.0, 0.0); 4];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        // Children placed side by side horizontally
        assert_eq!(results[1].x, 0.0);
        assert_eq!(results[2].x, 50.0);
        assert_eq!(results[3].x, 100.0);
        // All at same Y
        assert_eq!(results[1].y, 0.0);
        assert_eq!(results[2].y, 0.0);
        assert_eq!(results[3].y, 0.0);
    }

    #[test]
    fn flex_grow_distributes_space() {
        // Row container 300px wide; child1 fixed 100px, child2 flex-grow=1
        let nodes = vec![
            elem("div", None, vec![1, 2]),
            elem("div", Some(0), vec![]),
            elem("div", Some(0), vec![]),
        ];
        let styles = vec![
            style_with(|s| {
                s.display = Some("flex".into());
                s.flex_direction = Some("row".into());
                s.width = Some("300px".into());
                s.height = Some("100px".into());
            }),
            style_with(|s| {
                s.width = Some("100px".into());
                s.height = Some("50px".into());
            }),
            style_with(|s| {
                s.flex_grow = Some("1".into());
                s.height = Some("50px".into());
            }),
        ];
        let text_sizes = vec![(0.0, 0.0); 3];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        // First child: 100px
        assert_eq!(results[1].width, 100.0);
        // Second child: takes remaining 200px
        assert_eq!(results[2].width, 200.0);
        assert_eq!(results[2].x, 100.0);
    }

    #[test]
    fn justify_content_center() {
        // Row container 400px, two 50px children, centered
        let nodes = vec![
            elem("div", None, vec![1, 2]),
            elem("div", Some(0), vec![]),
            elem("div", Some(0), vec![]),
        ];
        let styles = vec![
            style_with(|s| {
                s.display = Some("flex".into());
                s.flex_direction = Some("row".into());
                s.justify_content = Some("center".into());
                s.width = Some("400px".into());
                s.height = Some("100px".into());
            }),
            style_with(|s| {
                s.width = Some("50px".into());
                s.height = Some("50px".into());
            }),
            style_with(|s| {
                s.width = Some("50px".into());
                s.height = Some("50px".into());
            }),
        ];
        let text_sizes = vec![(0.0, 0.0); 3];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        // Total children width = 100, container = 400, so offset = 150
        assert_eq!(results[1].x, 150.0);
        assert_eq!(results[2].x, 200.0);
    }

    #[test]
    fn position_fixed_absolute_coords() {
        // A root container + a fixed-position child
        let nodes = vec![
            elem("div", None, vec![1]),
            elem("div", Some(0), vec![]),
        ];
        let styles = vec![
            style_with(|s| {
                s.display = Some("flex".into());
                s.width = Some("1920px".into());
                s.height = Some("1080px".into());
            }),
            style_with(|s| {
                s.position = Some("fixed".into());
                s.top = Some("10px".into());
                s.left = Some("20px".into());
                s.width = Some("200px".into());
                s.height = Some("100px".into());
            }),
        ];
        let text_sizes = vec![(0.0, 0.0); 2];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        // Fixed element should be at (20, 10) absolute
        assert_eq!(results[1].x, 20.0);
        assert_eq!(results[1].y, 10.0);
        assert_eq!(results[1].width, 200.0);
        assert_eq!(results[1].height, 100.0);
    }

    #[test]
    fn text_sizing_respected() {
        // Parent span with a text child. Text measurement is on the parent
        // (the resolver measures at the span level, not the text node level).
        let nodes = vec![
            elem("span", None, vec![1]),
            text_node(Some(0)),
        ];
        let styles = vec![
            style_with(|s| {
                s.display = Some("flex".into());
            }),
            ResolvedStyle::default(),
        ];
        let text_sizes = vec![
            (80.0, 16.0),  // parent span — measured text dimensions
            (0.0, 0.0),    // text node — inherits parent's measurement
        ];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        // Parent span should be at least as wide as its text content
        assert!(results[0].width >= 80.0, "Span should be >= 80px wide, got {}", results[0].width);
        assert!(results[0].height >= 16.0, "Span should be >= 16px tall, got {}", results[0].height);
        // Text node inherits parent's measured size
        assert_eq!(results[1].width, 80.0);
        assert_eq!(results[1].height, 16.0);
    }

    #[test]
    fn gap_between_children() {
        // Column with gap=10 and 3 children each 20px tall
        let nodes = vec![
            elem("div", None, vec![1, 2, 3]),
            elem("div", Some(0), vec![]),
            elem("div", Some(0), vec![]),
            elem("div", Some(0), vec![]),
        ];
        let styles = vec![
            style_with(|s| {
                s.display = Some("flex".into());
                s.flex_direction = Some("column".into());
                s.gap = Some("10px".into());
                s.width = Some("200px".into());
            }),
            style_with(|s| {
                s.height = Some("20px".into());
            }),
            style_with(|s| {
                s.height = Some("20px".into());
            }),
            style_with(|s| {
                s.height = Some("20px".into());
            }),
        ];
        let text_sizes = vec![(0.0, 0.0); 4];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        // With gap=10: child1 at 0, child2 at 30, child3 at 60
        assert_eq!(results[1].y, 0.0);
        assert_eq!(results[2].y, 30.0);
        assert_eq!(results[3].y, 60.0);
    }

    #[test]
    fn padding_affects_children_position() {
        // Parent with padding=12, child at (0,0) relative to content area
        let nodes = vec![
            elem("div", None, vec![1]),
            elem("div", Some(0), vec![]),
        ];
        let styles = vec![
            style_with(|s| {
                s.display = Some("flex".into());
                s.padding = Some("12px".into());
                s.width = Some("200px".into());
                s.height = Some("100px".into());
            }),
            style_with(|s| {
                s.width = Some("50px".into());
                s.height = Some("30px".into());
            }),
        ];
        let text_sizes = vec![(0.0, 0.0); 2];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        // Child should be offset by parent's padding
        assert_eq!(results[1].x, 12.0);
        assert_eq!(results[1].y, 12.0);
    }

    #[test]
    fn empty_input_returns_empty() {
        let results = compute_layout(&[], &[], &[], 1920.0, 1080.0);
        assert!(results.is_empty());
    }

    #[test]
    fn nested_absolute_positions() {
        // Root > child_a (at y=0, h=50) > child_b (at relative y=0 inside child_a)
        // Absolute position of child_b should be root.y + child_a.y + child_b.y
        let nodes = vec![
            elem("div", None, vec![1]),
            elem("div", Some(0), vec![2]),
            elem("div", Some(1), vec![]),
        ];
        let styles = vec![
            style_with(|s| {
                s.display = Some("flex".into());
                s.flex_direction = Some("column".into());
                s.padding = Some("10px".into());
                s.width = Some("300px".into());
            }),
            style_with(|s| {
                s.display = Some("flex".into());
                s.flex_direction = Some("column".into());
                s.padding = Some("5px".into());
                s.height = Some("80px".into());
            }),
            style_with(|s| {
                s.width = Some("40px".into());
                s.height = Some("20px".into());
            }),
        ];
        let text_sizes = vec![(0.0, 0.0); 3];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        // child_a at (10, 10) due to root padding
        assert_eq!(results[1].x, 10.0);
        assert_eq!(results[1].y, 10.0);
        // child_b at (10 + 5, 10 + 5) = (15, 15) due to nested padding
        assert_eq!(results[2].x, 15.0);
        assert_eq!(results[2].y, 15.0);
    }
}

#[cfg(test)]
mod position_debug_test {
    use super::*;
    use crate::omni::types::ResolvedStyle;
    use crate::omni::flat_tree::FlatNode;

    #[test]
    fn fixed_position_with_left_top() {
        let nodes = vec![
            FlatNode {
                tag: "div".to_string(),
                id: None,
                classes: vec![],
                inline_style: None,
                conditional_classes: vec![],
                parent_index: None,
                depth: 0,
                is_text: false,
                text_content: None,
                child_indices: vec![1],
            },
            FlatNode {
                tag: "span".to_string(),
                id: None,
                classes: vec![],
                inline_style: None,
                conditional_classes: vec![],
                parent_index: Some(0),
                depth: 1,
                is_text: false,
                text_content: None,
                child_indices: vec![2],
            },
            FlatNode {
                tag: String::new(),
                id: None,
                classes: vec![],
                inline_style: None,
                conditional_classes: vec![],
                parent_index: Some(1),
                depth: 2,
                is_text: true,
                text_content: Some("test".to_string()),
                child_indices: vec![],
            },
        ];

        let mut root_style = ResolvedStyle::default();
        root_style.position = Some("fixed".to_string());
        root_style.top = Some("100px".to_string());
        root_style.left = Some("200px".to_string());
        root_style.width = Some("300px".to_string());
        root_style.display = Some("flex".to_string());
        root_style.flex_direction = Some("column".to_string());
        root_style.padding = Some("10px".to_string());

        let span_style = ResolvedStyle::default();
        let text_style = ResolvedStyle::default();

        let styles = vec![root_style, span_style, text_style];
        let text_sizes = vec![(0.0, 0.0), (80.0, 16.0), (0.0, 0.0)];

        let results = compute_layout(&nodes, &styles, &text_sizes, 1920.0, 1080.0);

        eprintln!("Root: x={}, y={}, w={}, h={}", results[0].x, results[0].y, results[0].width, results[0].height);
        eprintln!("Span: x={}, y={}, w={}, h={}", results[1].x, results[1].y, results[1].width, results[1].height);

        assert!(results[0].x > 100.0, "Root x should be ~200, got {}", results[0].x);
        assert!(results[0].y > 50.0, "Root y should be ~100, got {}", results[0].y);
    }
}
