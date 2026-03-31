//! CSS parsing and resolution for .omni files.
//!
//! Uses `lightningcss` for parsing CSS text into structured rules, then
//! performs our own selector matching against FlatNode trees. This hybrid
//! approach gives us robust CSS parsing (comments, nested braces, shorthand
//! expansion) while keeping selector matching simple and tailored to our
//! HtmlNode tree.

use std::collections::HashMap;

use super::flat_tree::{self, FlatNode};
use super::types::ResolvedStyle;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A parsed CSS rule with selector and specificity.
#[derive(Debug, Clone)]
pub struct CssRule {
    /// Original selector text (for debugging).
    pub selector_text: String,
    /// Parsed selector for matching against FlatNodes.
    pub selector: ParsedSelector,
    /// CSS property declarations (name -> value).
    pub properties: HashMap<String, String>,
    /// Specificity as (ids, classes, elements).
    pub specificity: (u32, u32, u32),
    /// Source order index for tie-breaking.
    pub source_order: usize,
}

/// A parsed selector that can be matched against FlatNodes.
#[derive(Debug, Clone)]
pub enum ParsedSelector {
    /// A single simple selector (e.g., `.panel`, `#fps`, `span`).
    Simple(SimpleSelector),
    /// A descendant selector chain, stored right-to-left:
    /// `[target, ancestor1, ancestor2, ...]`.
    /// E.g., `.panel .label` becomes `[.label, .panel]`.
    Descendant(Vec<SimpleSelector>),
    /// The `:root` pseudo-class (used only for variable extraction).
    Root,
}

/// A simple (possibly compound) selector that matches a single element.
#[derive(Debug, Clone)]
pub struct SimpleSelector {
    /// Tag/element name (e.g., "div", "span"). None = matches any element.
    pub element: Option<String>,
    /// ID (e.g., "fps"). None = no ID constraint.
    pub id: Option<String>,
    /// CSS classes that must ALL be present.
    pub classes: Vec<String>,
}

/// A parsed stylesheet: rules + CSS custom properties from `:root`.
#[derive(Debug, Clone, Default)]
pub struct ParsedStylesheet {
    pub rules: Vec<CssRule>,
    pub variables: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse a CSS string into rules and variables using lightningcss for
/// structural parsing, then our own selector parser for matching.
pub fn parse_css(source: &str) -> ParsedStylesheet {
    use lightningcss::rules::CssRule as LcssRule;
    use lightningcss::stylesheet::{ParserFlags, ParserOptions, StyleSheet};

    let options = ParserOptions {
        flags: ParserFlags::empty(),
        ..ParserOptions::default()
    };

    let sheet = match StyleSheet::parse(source, options) {
        Ok(s) => s,
        Err(_) => {
            // If lightningcss fails to parse, fall back to hand-written parser.
            return parse_css_fallback(source);
        }
    };

    let mut stylesheet = ParsedStylesheet::default();
    let mut order: usize = 0;

    for rule in &sheet.rules.0 {
        if let LcssRule::Style(style_rule) = rule {
            // Extract properties as name -> value strings.
            let properties = extract_properties(&style_rule.declarations);

            // Serialize the full selector list via Display (which is implemented
            // on SelectorList), then split by comma to get individual selectors.
            let selector_list_text = format!("{}", style_rule.selectors);
            let selector_texts: Vec<&str> = selector_list_text
                .split(',')
                .map(|s| s.trim())
                .collect();

            // Each selector in the SelectorList has its own specificity.
            for (i, selector) in style_rule.selectors.0.iter().enumerate() {
                let sel_text = selector_texts
                    .get(i)
                    .unwrap_or(&selector_list_text.as_str())
                    .to_string();

                let specificity_packed = selector.specificity();

                // Unpack specificity from the packed u32: a << 20 | b << 10 | c
                let a = (specificity_packed >> 20) & 0x3FF;
                let b = (specificity_packed >> 10) & 0x3FF;
                let c = specificity_packed & 0x3FF;
                let specificity = (a, b, c);

                let parsed_selector = parse_selector_text(&sel_text);

                // Extract :root variables.
                if matches!(&parsed_selector, ParsedSelector::Root) {
                    for (key, value) in &properties {
                        if key.starts_with("--") {
                            stylesheet.variables.insert(key.clone(), value.clone());
                        }
                    }
                }

                stylesheet.rules.push(CssRule {
                    selector_text: sel_text,
                    selector: parsed_selector,
                    properties: properties.clone(),
                    specificity,
                    source_order: order,
                });
                order += 1;
            }
        }
    }

    stylesheet
}

/// Extract property declarations from a lightningcss DeclarationBlock
/// as simple name -> value string pairs.
fn extract_properties(
    declarations: &lightningcss::declaration::DeclarationBlock,
) -> HashMap<String, String> {
    let mut props = HashMap::new();

    // Process normal declarations.
    for prop in &declarations.declarations {
        if let Some((name, value)) = property_to_kv(prop) {
            props.insert(name, value);
        }
    }

    // Process !important declarations (they still go in our map;
    // for our overlay use-case we don't distinguish importance).
    for prop in &declarations.important_declarations {
        if let Some((name, value)) = property_to_kv(prop) {
            props.insert(name, value);
        }
    }

    props
}

/// Convert a single lightningcss Property into a (name, value) pair.
fn property_to_kv(
    prop: &lightningcss::properties::Property,
) -> Option<(String, String)> {
    use lightningcss::printer::PrinterOptions;

    let name = prop.property_id().name().to_string();
    let value = prop.value_to_css_string(PrinterOptions::default()).ok()?;

    if !name.is_empty() && !value.is_empty() {
        Some((name, value))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Selector text parsing (our own, simpler than full CSS selector grammar)
// ---------------------------------------------------------------------------

/// Parse a selector text string into a ParsedSelector.
/// Supports: element, .class, #id, compound (.a.b), descendant (.a .b),
/// and :root.
fn parse_selector_text(text: &str) -> ParsedSelector {
    let text = text.trim();

    if text == ":root" {
        return ParsedSelector::Root;
    }

    // Split by whitespace to detect descendant combinators.
    // We do NOT support child (>), sibling (+, ~) combinators yet.
    let parts: Vec<&str> = text.split_whitespace().collect();

    if parts.is_empty() {
        return ParsedSelector::Root;
    }

    if parts.len() == 1 {
        return ParsedSelector::Simple(parse_simple_selector(parts[0]));
    }

    // Descendant selector: store right-to-left (target first, then ancestors).
    let selectors: Vec<SimpleSelector> = parts.iter().rev().map(|p| parse_simple_selector(p)).collect();
    ParsedSelector::Descendant(selectors)
}

/// Parse a single compound selector segment like `div.panel#main`.
fn parse_simple_selector(text: &str) -> SimpleSelector {
    let mut element = None;
    let mut id = None;
    let mut classes = Vec::new();

    // Tokenize by splitting on boundaries between element, #id, .class.
    let mut current = text;

    // Extract element name (starts at beginning, before any . or #).
    if !current.is_empty() && !current.starts_with('.') && !current.starts_with('#') {
        let end = current
            .find(|c: char| c == '.' || c == '#')
            .unwrap_or(current.len());
        let el = &current[..end];
        if !el.is_empty() {
            element = Some(el.to_lowercase());
        }
        current = &current[end..];
    }

    // Extract #id and .class tokens.
    while !current.is_empty() {
        if current.starts_with('#') {
            current = &current[1..];
            let end = current
                .find(|c: char| c == '.' || c == '#')
                .unwrap_or(current.len());
            let id_str = &current[..end];
            if !id_str.is_empty() {
                id = Some(id_str.to_string());
            }
            current = &current[end..];
        } else if current.starts_with('.') {
            current = &current[1..];
            let end = current
                .find(|c: char| c == '.' || c == '#')
                .unwrap_or(current.len());
            let cls = &current[..end];
            if !cls.is_empty() {
                classes.push(cls.to_string());
            }
            current = &current[end..];
        } else {
            break;
        }
    }

    SimpleSelector {
        element,
        id,
        classes,
    }
}

// ---------------------------------------------------------------------------
// Selector matching
// ---------------------------------------------------------------------------

/// Check if a SimpleSelector matches a FlatNode.
fn simple_matches(sel: &SimpleSelector, node: &FlatNode) -> bool {
    // Element constraint: must match tag.
    if let Some(ref el) = sel.element {
        if node.tag != *el {
            return false;
        }
    }

    // ID constraint: must match id.
    if let Some(ref sel_id) = sel.id {
        if node.id.as_deref() != Some(sel_id.as_str()) {
            return false;
        }
    }

    // All classes in the selector must be present on the node.
    for cls in &sel.classes {
        if !node.classes.iter().any(|c| c == cls) {
            return false;
        }
    }

    true
}

/// Check if a ParsedSelector matches a node at `node_index` in the flat tree.
fn selector_matches(
    selector: &ParsedSelector,
    node: &FlatNode,
    node_index: usize,
    flat_tree: &[FlatNode],
) -> bool {
    match selector {
        ParsedSelector::Root => false,
        ParsedSelector::Simple(simple) => simple_matches(simple, node),
        ParsedSelector::Descendant(chain) => {
            // chain[0] = target (rightmost), chain[1..] = ancestors (right-to-left).
            if chain.is_empty() {
                return false;
            }

            // The target (first in chain) must match the current node.
            if !simple_matches(&chain[0], node) {
                return false;
            }

            // Each remaining selector must match some ancestor, in order.
            // Walk up the ancestor chain for each required ancestor selector.
            let ancestor_selectors = &chain[1..];
            if ancestor_selectors.is_empty() {
                return true;
            }

            let ancestors = flat_tree::ancestor_chain(flat_tree, node_index);
            let mut ancestor_idx = 0;

            for ancestor_sel in ancestor_selectors {
                // Find the next ancestor that matches this selector.
                let mut found = false;
                while ancestor_idx < ancestors.len() {
                    let anc_node = &flat_tree[ancestors[ancestor_idx]];
                    ancestor_idx += 1;
                    if simple_matches(ancestor_sel, anc_node) {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return false;
                }
            }

            true
        }
    }
}

// ---------------------------------------------------------------------------
// Style resolution
// ---------------------------------------------------------------------------

/// Resolve styles for a FlatNode by matching CSS rules, applying specificity
/// ordering, and resolving var() references.
pub fn resolve_styles(
    node: &FlatNode,
    node_index: usize,
    flat_tree: &[FlatNode],
    stylesheet: &ParsedStylesheet,
    theme_vars: &HashMap<String, String>,
) -> ResolvedStyle {
    // Collect matching rules with their specificity and source order.
    let mut matching: Vec<&CssRule> = stylesheet
        .rules
        .iter()
        .filter(|rule| selector_matches(&rule.selector, node, node_index, flat_tree))
        .collect();

    // Sort by specificity (ascending), then source order. Later entries override earlier.
    matching.sort_by(|a, b| {
        a.specificity
            .cmp(&b.specificity)
            .then(a.source_order.cmp(&b.source_order))
    });

    // Merge properties: lower specificity first, higher overwrites.
    let mut merged: HashMap<String, String> = HashMap::new();
    for rule in &matching {
        for (key, value) in &rule.properties {
            merged.insert(key.clone(), value.clone());
        }
    }

    // Apply inline styles (highest priority, overrides everything).
    if let Some(ref inline) = node.inline_style {
        let inline_props = parse_inline_properties(inline);
        for (key, value) in inline_props {
            merged.insert(key, value);
        }
    }

    // Resolve var() references.
    // Cascade: theme vars first, then stylesheet :root vars (stylesheet wins on conflict,
    // because local :root overrides theme — same cascade order as CSS rules).
    let all_vars: HashMap<String, String> = theme_vars
        .iter()
        .chain(stylesheet.variables.iter())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    for value in merged.values_mut() {
        *value = resolve_variables(value, &all_vars);
    }

    props_to_resolved_style(&merged)
}

// ---------------------------------------------------------------------------
// Inline style parsing (simple key: value; pairs)
// ---------------------------------------------------------------------------

/// Parse inline style text into property key-value pairs.
fn parse_inline_properties(body: &str) -> HashMap<String, String> {
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

// ---------------------------------------------------------------------------
// var() resolution
// ---------------------------------------------------------------------------

/// Resolve `var(--name)` and `var(--name, fallback)` references in a CSS value.
fn resolve_variables(value: &str, variables: &HashMap<String, String>) -> String {
    let mut result = value.to_string();

    // Iterate a few times in case of chained references.
    for _ in 0..10 {
        if !result.contains("var(") {
            break;
        }

        if let Some(start) = result.find("var(") {
            let inner_start = start + 4;
            let mut depth = 1;
            let mut end = None;
            for (i, ch) in result[inner_start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(inner_start + i);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            let end = match end {
                Some(e) => e,
                None => break,
            };

            let inner = result[inner_start..end].trim();

            // Split on first comma for fallback support.
            let (name, fallback) = if let Some(comma_pos) = inner.find(',') {
                (
                    inner[..comma_pos].trim(),
                    Some(inner[comma_pos + 1..].trim()),
                )
            } else {
                (inner, None)
            };

            let replacement = if let Some(resolved) = variables.get(name) {
                resolved.clone()
            } else if let Some(fb) = fallback {
                fb.to_string()
            } else {
                // Unresolved, leave original.
                break;
            };

            result = format!("{}{}{}", &result[..start], replacement, &result[end + 1..]);
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Fallback parser (used when lightningcss cannot parse the input)
// ---------------------------------------------------------------------------

/// Simple hand-written CSS parser as fallback.
fn parse_css_fallback(source: &str) -> ParsedStylesheet {
    let mut stylesheet = ParsedStylesheet::default();
    let mut remaining = source.trim();
    let mut order: usize = 0;

    while !remaining.is_empty() {
        remaining = remaining.trim();
        if remaining.is_empty() {
            break;
        }

        let brace_open = match remaining.find('{') {
            Some(pos) => pos,
            None => break,
        };

        let selector_str = remaining[..brace_open].trim();

        let brace_close = match find_matching_brace(remaining, brace_open) {
            Some(pos) => pos,
            None => break,
        };

        let body = &remaining[brace_open + 1..brace_close];
        remaining = &remaining[brace_close + 1..];

        let selector = parse_selector_text(selector_str);
        let properties = parse_inline_properties(body);

        // Compute specificity from our parsed selector.
        let specificity = compute_specificity(&selector);

        if matches!(&selector, ParsedSelector::Root) {
            for (key, value) in &properties {
                if key.starts_with("--") {
                    stylesheet.variables.insert(key.clone(), value.clone());
                }
            }
        }

        stylesheet.rules.push(CssRule {
            selector_text: selector_str.to_string(),
            selector,
            properties,
            specificity,
            source_order: order,
        });
        order += 1;
    }

    stylesheet
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

/// Compute specificity from a ParsedSelector.
fn compute_specificity(selector: &ParsedSelector) -> (u32, u32, u32) {
    match selector {
        ParsedSelector::Root => (0, 0, 0),
        ParsedSelector::Simple(s) => simple_specificity(s),
        ParsedSelector::Descendant(chain) => {
            let mut total = (0u32, 0u32, 0u32);
            for s in chain {
                let sp = simple_specificity(s);
                total.0 += sp.0;
                total.1 += sp.1;
                total.2 += sp.2;
            }
            total
        }
    }
}

fn simple_specificity(s: &SimpleSelector) -> (u32, u32, u32) {
    let ids = if s.id.is_some() { 1 } else { 0 };
    let classes = s.classes.len() as u32;
    let elements = if s.element.is_some() { 1 } else { 0 };
    (ids, classes, elements)
}

// ---------------------------------------------------------------------------
// ResolvedStyle conversion
// ---------------------------------------------------------------------------

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
        min_width: props.get("min-width").cloned(),
        max_width: props.get("max-width").cloned(),
        min_height: props.get("min-height").cloned(),
        max_height: props.get("max-height").cloned(),
        background_color: props.get("background-color").cloned(),
        box_shadow: props.get("box-shadow").cloned(),
        align_self: props.get("align-self").cloned(),
        flex_grow: props.get("flex-grow").cloned(),
        flex_shrink: props.get("flex-shrink").cloned(),
        flex_wrap: props.get("flex-wrap").cloned(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::flat_tree::flatten_tree;
    use super::super::types::HtmlNode;

    /// Helper: build a flat tree from an HtmlNode and return (flat_tree, index_map).
    fn make_test_tree() -> (Vec<FlatNode>, HtmlNode) {
        // <div class="panel">
        //   <div class="row">
        //     <span class="value critical" id="cpu">text</span>
        //   </div>
        //   <span class="label">label text</span>
        // </div>
        let tree = HtmlNode::Element {
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
                    children: vec![HtmlNode::Element {
                        tag: "span".to_string(),
                        id: Some("cpu".to_string()),
                        classes: vec!["value".to_string(), "critical".to_string()],
                        inline_style: Some("color: red".to_string()),
                        children: vec![HtmlNode::Text {
                            content: "text".to_string(),
                        }],
                    }],
                },
                HtmlNode::Element {
                    tag: "span".to_string(),
                    id: None,
                    classes: vec!["label".to_string()],
                    inline_style: None,
                    children: vec![HtmlNode::Text {
                        content: "label text".to_string(),
                    }],
                },
            ],
        };
        let flat = flatten_tree(&tree);
        (flat, tree)
    }

    // --- Test 1: Simple class selector matching ---
    #[test]
    fn simple_class_selector() {
        let (flat, _) = make_test_tree();
        // lightningcss normalizes "white" -> "#fff", so we test with hex.
        let css = ".label { color: #ffffff; font-size: 14px; }";
        let sheet = parse_css(css);
        assert_eq!(sheet.rules.len(), 1);

        // span.label is at index 4 (no inline style)
        let style = resolve_styles(&flat[4], 4, &flat, &sheet, &HashMap::new());
        assert_eq!(style.color.as_deref(), Some("#fff"));
        assert_eq!(style.font_size.as_deref(), Some("14px"));

        // div.panel at index 0 should NOT match .label
        let style0 = resolve_styles(&flat[0], 0, &flat, &sheet, &HashMap::new());
        assert!(style0.color.is_none());
    }

    // --- Test 2: ID selector matching ---
    #[test]
    fn id_selector() {
        let (flat, _) = make_test_tree();
        // Test with font-weight to avoid inline color on #cpu.
        let css = "#cpu { font-weight: bold; }";
        let sheet = parse_css(css);

        // span#cpu is at index 2
        let style = resolve_styles(&flat[2], 2, &flat, &sheet, &HashMap::new());
        assert_eq!(style.font_weight.as_deref(), Some("bold"));

        // span.label (index 4) should NOT match #cpu
        let style4 = resolve_styles(&flat[4], 4, &flat, &sheet, &HashMap::new());
        assert!(style4.font_weight.is_none());
    }

    // --- Test 3: Descendant selector matching ---
    #[test]
    fn descendant_selector() {
        let (flat, _) = make_test_tree();
        let css = ".panel .label { font-size: 18px; }";
        let sheet = parse_css(css);

        // span.label (index 4) is a descendant of div.panel (index 0)
        let style = resolve_styles(&flat[4], 4, &flat, &sheet, &HashMap::new());
        assert_eq!(style.font_size.as_deref(), Some("18px"));

        // div.panel (index 0) itself should NOT match .panel .label
        let style0 = resolve_styles(&flat[0], 0, &flat, &sheet, &HashMap::new());
        assert!(style0.font_size.is_none());
    }

    // --- Test 4: Compound selector matching ---
    #[test]
    fn compound_selector() {
        let (flat, _) = make_test_tree();
        // Test with font-size to avoid inline color conflict on index 2.
        let css = ".value.critical { font-size: 24px; }";
        let sheet = parse_css(css);

        // span.value.critical (index 2) has both classes
        let style = resolve_styles(&flat[2], 2, &flat, &sheet, &HashMap::new());
        assert_eq!(style.font_size.as_deref(), Some("24px"));

        // span.label (index 4) only has "label", not both "value" and "critical"
        let style4 = resolve_styles(&flat[4], 4, &flat, &sheet, &HashMap::new());
        assert!(style4.font_size.is_none());
    }

    // --- Test 5: Specificity ordering ---
    #[test]
    fn specificity_ordering() {
        let (flat, _) = make_test_tree();
        // Use font-size to avoid inline color on index 2.
        // Source order: element first, then class, then ID.
        // ID should win regardless of source order.
        let css = r#"
            span { font-size: 10px; }
            .value { font-size: 14px; }
            #cpu { font-size: 20px; }
        "#;
        let sheet = parse_css(css);

        // span#cpu.value.critical (index 2): all three rules match.
        // ID specificity (1,0,0) > class (0,1,0) > element (0,0,1).
        let style = resolve_styles(&flat[2], 2, &flat, &sheet, &HashMap::new());
        assert_eq!(style.font_size.as_deref(), Some("20px"));
    }

    // --- Test 5b: Higher specificity wins regardless of source order ---
    #[test]
    fn specificity_beats_source_order() {
        let (flat, _) = make_test_tree();
        // ID rule appears FIRST in source, class rule LAST.
        // ID should still win because specificity > source order.
        let css = r#"
            #cpu { font-size: 20px; }
            .value { font-size: 14px; }
        "#;
        let sheet = parse_css(css);

        let style = resolve_styles(&flat[2], 2, &flat, &sheet, &HashMap::new());
        assert_eq!(style.font_size.as_deref(), Some("20px"));
    }

    // --- Test 6: var() resolution ---
    #[test]
    fn var_resolution() {
        let (flat, _) = make_test_tree();
        let css = ":root { --bg: red; } .panel { background: var(--bg); }";
        let sheet = parse_css(css);

        let style = resolve_styles(&flat[0], 0, &flat, &sheet, &HashMap::new());
        assert_eq!(style.background.as_deref(), Some("red"));
    }

    // --- Test 7: Theme variables apply ---
    #[test]
    fn theme_variables() {
        let (flat, _) = make_test_tree();
        let css = ".label { color: var(--text); }";
        let sheet = parse_css(css);

        let mut theme_vars = HashMap::new();
        theme_vars.insert("--text".to_string(), "#ffffff".to_string());

        let style = resolve_styles(&flat[4], 4, &flat, &sheet, &theme_vars);
        assert_eq!(style.color.as_deref(), Some("#ffffff"));
    }

    // --- Test 8: Inline style overrides all ---
    #[test]
    fn inline_style_overrides() {
        let (flat, _) = make_test_tree();
        // CSS rule sets color to green for #cpu, but inline style says "color: red".
        let css = "#cpu { color: green; }";
        let sheet = parse_css(css);

        // span#cpu (index 2) has inline_style = "color: red"
        let style = resolve_styles(&flat[2], 2, &flat, &sheet, &HashMap::new());
        assert_eq!(style.color.as_deref(), Some("red"));
    }

    // --- Additional: parse_css produces correct variables ---
    #[test]
    fn parse_root_variables() {
        let css = ":root { --bg: rgba(20,20,20,0.7); --text: #ffffff; }";
        let sheet = parse_css(css);
        // lightningcss normalizes custom property values:
        // rgba(20,20,20,0.7) -> #141414b3, #ffffff -> #fff
        let bg = sheet.variables.get("--bg").unwrap();
        assert!(!bg.is_empty(), "Expected --bg to have a value, got empty");
        let text = sheet.variables.get("--text").unwrap();
        assert!(!text.is_empty(), "Expected --text to have a value, got empty");
        // The exact format depends on lightningcss version, but both should be present.
        assert_eq!(sheet.variables.len(), 2);
    }

    // --- Descendant with deeper nesting ---
    #[test]
    fn descendant_selector_deep() {
        let (flat, _) = make_test_tree();
        // .panel .value should match span.value.critical (index 2)
        // because .panel is an ancestor (grandparent via div.row).
        let css = ".panel .value { font-size: 20px; }";
        let sheet = parse_css(css);

        let style = resolve_styles(&flat[2], 2, &flat, &sheet, &HashMap::new());
        assert_eq!(style.font_size.as_deref(), Some("20px"));
    }

    // --- Multiple rules, same specificity, source order wins ---
    #[test]
    fn source_order_tiebreak() {
        let (flat, _) = make_test_tree();
        // Use font-size to avoid inline color conflict.
        let css = r#"
            .value { font-size: 10px; }
            .critical { font-size: 20px; }
        "#;
        let sheet = parse_css(css);

        // Both .value and .critical match index 2 with same specificity (0,1,0).
        // .critical comes later in source order, so it wins.
        let style = resolve_styles(&flat[2], 2, &flat, &sheet, &HashMap::new());
        assert_eq!(style.font_size.as_deref(), Some("20px"));
    }
}
