//! Validation and suggestion engine for .omni files.
//!
//! Provides edit-distance-based fuzzy matching for element names,
//! CSS properties, and sensor paths.

use super::parser::ParseError;

/// Known element names supported in .omni templates. Includes HTML (div,
/// span, i) plus the SVG subset used by the charting system.
pub const KNOWN_ELEMENTS: &[&str] = &[
    // HTML
    "div", "span", "i",
    // SVG container and shapes used by <chart> / <chart-card> desugaring
    "svg", "g", "polyline", "path", "rect", "circle", "line", "text", "ellipse",
    "polygon", "defs", "linearGradient", "radialGradient", "stop",
];

/// Known sensor paths for interpolation expressions.
pub const KNOWN_SENSOR_PATHS: &[&str] = &[
    "cpu.usage",
    "cpu.temp",
    "gpu.usage",
    "gpu.temp",
    "gpu.clock",
    "gpu.mem-clock",
    "gpu.vram",
    "gpu.vram.used",
    "gpu.vram.total",
    "gpu.power",
    "gpu.fan",
    "ram.usage",
    "ram.used",
    "ram.total",
    "fps",
    "frame-time",
    "frame-time.avg",
    "frame-time.1pct",
    "frame-time.01pct",
];

/// Compute the Levenshtein edit distance between two strings.
#[allow(clippy::needless_range_loop)]
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
            let cost = if a_bytes[i - 1] == b_bytes[j - 1] {
                0
            } else {
                1
            };
            matrix[i][j] = (matrix[i - 1][j] + 1) // deletion
                .min(matrix[i][j - 1] + 1) // insertion
                .min(matrix[i - 1][j - 1] + cost); // substitution
        }
    }

    matrix[a_len][b_len]
}

/// Find the best match for `input` in a list of known values.
/// Returns None if no match is within `max_distance` edits.
pub fn suggest(input: &str, known: &[&str], max_distance: usize) -> Option<String> {
    let input_lower = input.to_lowercase();

    // If input exactly matches a known value (case-insensitive), it's not a typo
    if known.iter().any(|&k| k.eq_ignore_ascii_case(&input_lower)) {
        return None;
    }

    let mut best: Option<(usize, &str)> = None;

    for &candidate in known {
        let dist = edit_distance(&input_lower, &candidate.to_lowercase());
        if dist > 0 && dist <= max_distance && (best.is_none() || dist < best.unwrap().0) {
            best = Some((dist, candidate));
        }
    }

    best.map(|(_, s)| s.to_string())
}

/// Suggest an element name. Returns e.g., "did you mean <div>?"
pub fn suggest_element(unknown: &str) -> Option<String> {
    suggest(unknown, KNOWN_ELEMENTS, 2).map(|s| format!("did you mean <{}>?", s))
}

/// Suggest a sensor path. Returns e.g., "did you mean \"gpu.temp\"?"
pub fn suggest_sensor_path(unknown: &str) -> Option<String> {
    suggest(unknown, KNOWN_SENSOR_PATHS, 3).map(|s| format!("did you mean \"{}\"?", s))
}

/// Validate sensor paths found in template text (inside {}).
///
/// NOTE: Currently uses old ParseError format (message + offset). Task 3 will
/// reconcile once ParseError gains line/column/severity/suggestion fields.
/// Validate sensor paths found in template text (inside {}).
///
/// Like `validate_sensor_paths`, but when `hwinfo_connected` is `false` and an
/// `hwinfo.*` path is encountered, emits a single warning for the whole file:
/// "HWiNFO is not running — hwinfo.* sensors will show N/A".
/// Only one such warning is emitted per call, regardless of how many hwinfo paths exist.
/// When `hwinfo_connected` is `true`, hwinfo paths are silently skipped (valid).
pub fn validate_sensor_paths_with_hwinfo(
    template_text: &str,
    omni_source: &str,
    text_offset: usize,
    hwinfo_connected: bool,
) -> Vec<ParseError> {
    use super::sensor_map::parse_sensor_path;

    let mut warnings = Vec::new();
    let mut chars = template_text.char_indices().peekable();
    let mut hwinfo_warned = false;

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
                // Function-call interpolation (e.g. `chart_polyline(cpu.usage, 200, 60)`,
                // `nice_tick(sensor, unit, i, n)`, `format_value(...)`). These are
                // handled by the interpolation dispatcher at runtime — skip sensor-path
                // validation. Sensor paths with a precision suffix (e.g. `cpu.usage(2)`)
                // contain a `.` before the `(`, so we can distinguish them from
                // function calls which use bare identifiers.
                if let Some(paren_idx) = path.find('(') {
                    if path.ends_with(')') && !path[..paren_idx].contains('.') {
                        continue;
                    }
                }
                if path.starts_with("hwinfo.") {
                    if !hwinfo_connected && !hwinfo_warned {
                        hwinfo_warned = true;
                        let offset = text_offset + start;
                        let (line, column) = super::parser::offset_to_line_col(omni_source, offset);
                        warnings.push(ParseError {
                            message:
                                "HWiNFO is not running \u{2014} hwinfo.* sensors will show N/A"
                                    .to_string(),
                            severity: super::parser::Severity::Warning,
                            line,
                            column,
                            suggestion: None,
                        });
                    }
                    continue;
                }
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
        assert_eq!(edit_distance("div", "di"), 1); // deletion
        assert_eq!(edit_distance("div", "divv"), 1); // insertion
        assert_eq!(edit_distance("div", "dib"), 1); // substitution
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
        assert_eq!(
            suggest("gup.temp", KNOWN_SENSOR_PATHS, 3),
            Some("gpu.temp".to_string())
        );
        assert_eq!(suggest("zzzzzzzzzzz", KNOWN_SENSOR_PATHS, 3), None);
    }

    #[test]
    fn suggest_exact_match_returns_none() {
        assert_eq!(suggest("div", KNOWN_ELEMENTS, 2), None);
        assert_eq!(suggest("ram.used", KNOWN_SENSOR_PATHS, 3), None);
    }

    #[test]
    fn suggest_near_miss_case_insensitive() {
        assert_eq!(
            suggest("DIV", KNOWN_ELEMENTS, 2),
            None // exact match case-insensitive
        );
        assert_eq!(suggest("DVI", KNOWN_ELEMENTS, 2), Some("div".to_string()));
    }

    #[test]
    fn suggest_element_typo() {
        assert_eq!(
            suggest_element("dvi"),
            Some("did you mean <div>?".to_string())
        );
        assert_eq!(
            suggest_element("sapn"),
            Some("did you mean <span>?".to_string())
        );
        assert_eq!(suggest_element("completely_wrong"), None);
    }

    #[test]
    fn suggest_sensor_path_typo() {
        assert_eq!(
            suggest_sensor_path("gpu.tamp"),
            Some("did you mean \"gpu.temp\"?".to_string())
        );
        assert_eq!(
            suggest_sensor_path("cpu.usag"),
            Some("did you mean \"cpu.usage\"?".to_string())
        );
        assert_eq!(suggest_sensor_path("totally.fake.path"), None);
    }

    #[test]
    fn validate_sensor_path_unknown() {
        let text = "CPU: {cpu.usag}%";
        let warnings = validate_sensor_paths_with_hwinfo(text, text, 0, true);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("cpu.usag"));
        assert!(warnings[0].message.contains("cpu.usage"));
    }

    #[test]
    fn validate_sensor_path_valid() {
        let text = "CPU: {cpu.usage}%";
        let warnings = validate_sensor_paths_with_hwinfo(text, text, 0, true);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn validate_sensor_path_multiple() {
        let text = "{gpu.tamp} / {cpu.usag}";
        let warnings = validate_sensor_paths_with_hwinfo(text, text, 0, true);
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn validate_hwinfo_path_warns_when_disconnected() {
        let text = "VRM: {hwinfo.motherboard.vrm_temp}°C";
        let warnings = validate_sensor_paths_with_hwinfo(text, text, 0, false);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("HWiNFO is not running"));
    }

    #[test]
    fn validate_hwinfo_path_no_warn_when_connected() {
        let text = "VRM: {hwinfo.motherboard.vrm_temp}°C";
        let warnings = validate_sensor_paths_with_hwinfo(text, text, 0, true);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn validate_hwinfo_only_one_warning_for_multiple_paths() {
        let text = "{hwinfo.cpu.core_0_temp} / {hwinfo.gpu.vrm_temp}";
        let warnings = validate_sensor_paths_with_hwinfo(text, text, 0, false);
        assert_eq!(warnings.len(), 1); // Only one warning, not two
    }
}
