//! CSS parsing and resolution for .omni files.
//!
//! Uses a hand-written CSS parser for the subset we need in Phase 9a-1.
//! `lightningcss` will be integrated in Phase 9a-2 for the full CSS grammar.

use std::collections::HashMap;

use super::types::ResolvedStyle;

/// A parsed CSS rule: selector -> properties.
#[derive(Debug, Clone)]
pub struct CssRule {
    pub selector: Selector,
    pub properties: HashMap<String, String>,
}

/// A simple CSS selector (Phase 9a-1: class or ID only).
#[derive(Debug, Clone)]
pub enum Selector {
    Class(String),   // .panel
    Id(String),      // #fps
    Element(String), // div, span
    Root,            // :root (for variables)
}

/// Parsed stylesheet: rules + variables.
#[derive(Debug, Clone, Default)]
pub struct ParsedStylesheet {
    pub rules: Vec<CssRule>,
    pub variables: HashMap<String, String>,
}

/// Parse a CSS string into rules and variables.
pub fn parse_css(source: &str) -> ParsedStylesheet {
    let mut stylesheet = ParsedStylesheet::default();
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

        let selector = parse_selector(selector_str);
        let properties = parse_properties(body);

        // Extract :root variables
        if matches!(&selector, Selector::Root) {
            for (key, value) in &properties {
                if key.starts_with("--") {
                    stylesheet.variables.insert(key.clone(), value.clone());
                }
            }
        }

        stylesheet.rules.push(CssRule {
            selector,
            properties,
        });
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

    // Apply element selectors first (lowest priority)
    for rule in &stylesheet.rules {
        if matches!(&rule.selector, Selector::Element(el) if el == tag) {
            for (key, value) in &rule.properties {
                merged.insert(key.clone(), value.clone());
            }
        }
    }

    // Apply class selectors (medium priority)
    for rule in &stylesheet.rules {
        if matches!(&rule.selector, Selector::Class(cls) if classes.iter().any(|c| c == cls)) {
            for (key, value) in &rule.properties {
                merged.insert(key.clone(), value.clone());
            }
        }
    }

    // Apply ID selectors (high priority)
    for rule in &stylesheet.rules {
        if matches!(&rule.selector, Selector::Id(rule_id) if id.map_or(false, |eid| eid == rule_id.as_str()))
        {
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

/// Resolve `var(--name)` and `var(--name, fallback)` references in a CSS value.
fn resolve_variables(value: &str, variables: &HashMap<String, String>) -> String {
    let mut result = value.to_string();

    // Iterate a few times in case of chained references
    for _ in 0..10 {
        if !result.contains("var(") {
            break;
        }

        if let Some(start) = result.find("var(") {
            // Find the matching closing paren
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

            // Split on first comma for fallback support
            let (name, fallback) = if let Some(comma_pos) = inner.find(',') {
                (inner[..comma_pos].trim(), Some(inner[comma_pos + 1..].trim()))
            } else {
                (inner, None)
            };

            let replacement = if let Some(resolved) = variables.get(name) {
                resolved.clone()
            } else if let Some(fb) = fallback {
                fb.to_string()
            } else {
                // Unresolved, leave original
                break;
            };

            result = format!("{}{}{}", &result[..start], replacement, &result[end + 1..]);
        }
    }

    result
}

fn parse_selector(s: &str) -> Selector {
    let s = s.trim();
    if s == ":root" {
        Selector::Root
    } else if let Some(rest) = s.strip_prefix('#') {
        Selector::Id(rest.to_string())
    } else if let Some(rest) = s.strip_prefix('.') {
        Selector::Class(rest.to_string())
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
            "div",
            None,
            &["panel".to_string()],
            None,
            &sheet,
            &HashMap::new(),
        );
        assert_eq!(style.color.as_deref(), Some("white"));
        assert!(style.font_size.is_none()); // .value doesn't match
    }

    #[test]
    fn resolve_id_selector() {
        let css = "#fps { color: green; }";
        let sheet = parse_css(css);
        let style = resolve_styles(
            "span",
            Some("fps"),
            &[],
            None,
            &sheet,
            &HashMap::new(),
        );
        assert_eq!(style.color.as_deref(), Some("green"));
    }

    #[test]
    fn inline_style_overrides() {
        let css = ".panel { color: white; }";
        let sheet = parse_css(css);
        let style = resolve_styles(
            "div",
            None,
            &["panel".to_string()],
            Some("color: red; font-size: 20px;"),
            &sheet,
            &HashMap::new(),
        );
        assert_eq!(style.color.as_deref(), Some("red"));
        assert_eq!(style.font_size.as_deref(), Some("20px"));
    }

    #[test]
    fn variable_resolution() {
        let css = ":root { --bg: red; } .panel { background: var(--bg); }";
        let sheet = parse_css(css);
        let style = resolve_styles(
            "div",
            None,
            &["panel".to_string()],
            None,
            &sheet,
            &HashMap::new(),
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
            "div",
            None,
            &["panel".to_string()],
            None,
            &sheet,
            &theme_vars,
        );
        assert_eq!(style.color.as_deref(), Some("#ffffff"));
    }
}
