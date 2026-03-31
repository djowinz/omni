//! Validation and suggestion engine for .omni files.
//!
//! Provides edit-distance-based fuzzy matching for element names,
//! CSS properties, and sensor paths.

use super::parser::ParseError;

/// Known HTML element names supported in .omni templates.
pub const KNOWN_ELEMENTS: &[&str] = &["div", "span"];

/// Known CSS properties supported by the resolver.
pub const KNOWN_CSS_PROPERTIES: &[&str] = &[
    "position", "top", "right", "bottom", "left",
    "width", "height", "min-width", "max-width", "min-height", "max-height",
    "background", "background-color", "color", "opacity", "border-radius",
    "box-shadow", "border-width", "border-color",
    "font-size", "font-weight", "font-family",
    "display", "flex-direction", "justify-content", "align-items",
    "align-self", "flex-grow", "flex-shrink", "flex-wrap", "gap",
    "padding", "margin", "transition",
];

/// Known sensor paths for interpolation expressions.
pub const KNOWN_SENSOR_PATHS: &[&str] = &[
    "cpu.usage", "cpu.temp",
    "gpu.usage", "gpu.temp", "gpu.clock", "gpu.mem-clock",
    "gpu.vram", "gpu.vram.used", "gpu.vram.total",
    "gpu.power", "gpu.fan",
    "ram.usage", "ram.used", "ram.total",
    "fps", "frame-time", "frame-time.avg",
    "frame-time.1pct", "frame-time.01pct",
];

/// Compute the Levenshtein edit distance between two strings.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a_len = a.len();
    let b_len = b.len();
    let mut matrix = vec![vec![0usize; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] { 0 } else { 1 };
            matrix[i][j] = (matrix[i - 1][j] + 1)           // deletion
                .min(matrix[i][j - 1] + 1)                    // insertion
                .min(matrix[i - 1][j - 1] + cost);            // substitution
        }
    }

    matrix[a_len][b_len]
}

/// Find the best match for `input` in a list of known values.
/// Returns None if no match is within `max_distance` edits.
pub fn suggest(input: &str, known: &[&str], max_distance: usize) -> Option<String> {
    let input_lower = input.to_lowercase();
    let mut best: Option<(usize, &str)> = None;

    for &candidate in known {
        let dist = edit_distance(&input_lower, &candidate.to_lowercase());
        if dist <= max_distance {
            if best.is_none() || dist < best.unwrap().0 {
                best = Some((dist, candidate));
            }
        }
    }

    best.map(|(_, s)| s.to_string())
}

/// Suggest an element name. Returns e.g., "did you mean <div>?"
pub fn suggest_element(unknown: &str) -> Option<String> {
    suggest(unknown, KNOWN_ELEMENTS, 2)
        .map(|s| format!("did you mean <{}>?", s))
}

/// Suggest a CSS property. Returns e.g., "did you mean \"color\"?"
pub fn suggest_css_property(unknown: &str) -> Option<String> {
    suggest(unknown, KNOWN_CSS_PROPERTIES, 2)
        .map(|s| format!("did you mean \"{}\"?", s))
}

/// Suggest a sensor path. Returns e.g., "did you mean \"gpu.temp\"?"
pub fn suggest_sensor_path(unknown: &str) -> Option<String> {
    suggest(unknown, KNOWN_SENSOR_PATHS, 3)
        .map(|s| format!("did you mean \"{}\"?", s))
}

/// Validate CSS properties in a style source and return warnings for unknown ones.
///
/// NOTE: Currently uses old ParseError format (message + offset). Task 3 will
/// reconcile once ParseError gains line/column/severity/suggestion fields.
pub fn validate_css_properties(
    css_source: &str,
    omni_source: &str,
    base_offset: usize,
) -> Vec<ParseError> {
    let mut warnings = Vec::new();

    for line_str in css_source.lines() {
        let trimmed = line_str.trim();
        if trimmed.is_empty() || trimmed.starts_with('}') || trimmed.starts_with('{')
            || trimmed.starts_with('.') || trimmed.starts_with('#')
            || trimmed.starts_with(':') || trimmed.starts_with('@') {
            continue;
        }

        if let Some(colon_pos) = trimmed.find(':') {
            let prop_name = trimmed[..colon_pos].trim();
            if prop_name.starts_with('-') {
                continue; // CSS custom properties (--var)
            }
            if !prop_name.is_empty() && !KNOWN_CSS_PROPERTIES.contains(&prop_name) {
                let suggestion = suggest_css_property(prop_name);
                // Find offset in omni source
                let prop_offset = if let Some(pos) = omni_source.find(trimmed) {
                    pos
                } else {
                    base_offset
                };
                let msg = match suggestion {
                    Some(ref s) => format!("unsupported CSS property \"{}\"; {}", prop_name, s),
                    None => format!("unsupported CSS property \"{}\"", prop_name),
                };
                let (line, column) = super::parser::offset_to_line_col(omni_source, prop_offset);
                warnings.push(ParseError {
                    message: msg,
                    severity: super::parser::Severity::Warning,
                    line,
                    column,
                    suggestion,
                });
            }
        }
    }

    warnings
}

/// Validate sensor paths found in template text (inside {}).
///
/// NOTE: Currently uses old ParseError format (message + offset). Task 3 will
/// reconcile once ParseError gains line/column/severity/suggestion fields.
pub fn validate_sensor_paths(
    template_text: &str,
    omni_source: &str,
    text_offset: usize,
) -> Vec<ParseError> {
    use super::sensor_map::parse_sensor_path;

    let mut warnings = Vec::new();
    let mut chars = template_text.char_indices().peekable();

    while let Some((i, ch)) = chars.next() {
        if ch == '{' {
            let start = i + 1;
            let mut path = String::new();
            let mut found_close = false;
            for (_, inner) in chars.by_ref() {
                if inner == '}' {
                    found_close = true;
                    break;
                }
                path.push(inner);
            }
            if found_close && !path.is_empty() {
                let path = path.trim();
                if parse_sensor_path(path).is_none() {
                    let suggestion = suggest_sensor_path(path);
                    let msg = match suggestion {
                        Some(ref s) => format!("unknown sensor path \"{}\"; {}", path, s),
                        None => format!("unknown sensor path \"{}\"", path),
                    };
                    let offset = text_offset + start;
                    let (line, column) = super::parser::offset_to_line_col(omni_source, offset);
                    warnings.push(ParseError {
                        message: msg,
                        severity: super::parser::Severity::Warning,
                        line,
                        column,
                        suggestion,
                    });
                }
            }
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_identical() {
        assert_eq!(edit_distance("hello", "hello"), 0);
    }

    #[test]
    fn edit_distance_one_char() {
        assert_eq!(edit_distance("div", "dvi"), 2); // transposition = 2 edits
        assert_eq!(edit_distance("div", "di"), 1);   // deletion
        assert_eq!(edit_distance("div", "divv"), 1);  // insertion
        assert_eq!(edit_distance("div", "dib"), 1);   // substitution
    }

    #[test]
    fn edit_distance_empty() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
    }

    #[test]
    fn edit_distance_both_empty() {
        assert_eq!(edit_distance("", ""), 0);
    }

    #[test]
    fn suggest_finds_best_match() {
        assert_eq!(suggest("colr", KNOWN_CSS_PROPERTIES, 2), Some("color".to_string()));
        assert_eq!(suggest("zzzzzzzzzzz", KNOWN_CSS_PROPERTIES, 2), None);
    }

    #[test]
    fn suggest_case_insensitive() {
        assert_eq!(suggest("COLOR", KNOWN_CSS_PROPERTIES, 2), Some("color".to_string()));
        assert_eq!(suggest("DIV", KNOWN_ELEMENTS, 2), Some("div".to_string()));
    }

    #[test]
    fn suggest_element_typo() {
        assert_eq!(suggest_element("dvi"), Some("did you mean <div>?".to_string()));
        assert_eq!(suggest_element("sapn"), Some("did you mean <span>?".to_string()));
        assert_eq!(suggest_element("completely_wrong"), None);
    }

    #[test]
    fn suggest_css_property_typo() {
        assert_eq!(suggest_css_property("colr"), Some("did you mean \"color\"?".to_string()));
        assert_eq!(suggest_css_property("backgroud"), Some("did you mean \"background\"?".to_string()));
        assert_eq!(suggest_css_property("font-sie"), Some("did you mean \"font-size\"?".to_string()));
    }

    #[test]
    fn suggest_sensor_path_typo() {
        assert_eq!(suggest_sensor_path("gpu.tamp"), Some("did you mean \"gpu.temp\"?".to_string()));
        assert_eq!(suggest_sensor_path("cpu.usag"), Some("did you mean \"cpu.usage\"?".to_string()));
        assert_eq!(suggest_sensor_path("totally.fake.path"), None);
    }

    #[test]
    fn validate_css_finds_unknown_property() {
        let css = ".panel {\n  colr: red;\n  font-size: 14px;\n}";
        let omni = css;
        let warnings = validate_css_properties(css, omni, 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("colr"));
        assert!(warnings[0].message.contains("did you mean \"color\"?"));
    }

    #[test]
    fn validate_css_ignores_custom_properties() {
        let css = ":root {\n  --bg: red;\n  --text: white;\n}";
        let warnings = validate_css_properties(css, css, 0);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn validate_sensor_path_unknown() {
        let text = "CPU: {cpu.usag}%";
        let warnings = validate_sensor_paths(text, text, 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("cpu.usag"));
        assert!(warnings[0].message.contains("cpu.usage"));
    }

    #[test]
    fn validate_sensor_path_valid() {
        let text = "CPU: {cpu.usage}%";
        let warnings = validate_sensor_paths(text, text, 0);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn validate_sensor_path_multiple() {
        let text = "{gpu.tamp} / {cpu.usag}";
        let warnings = validate_sensor_paths(text, text, 0);
        assert_eq!(warnings.len(), 2);
    }
}
