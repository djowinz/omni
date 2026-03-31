# Phase 9a-4: Structured Error Reporting

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **IMPORTANT: Run superpowers:code-reviewer after EVERY subagent task. No exceptions.**

**Goal:** Expand parser errors with line/column numbers, severity levels, and fuzzy-match suggestions so the Electron app's Monaco editor can display inline squiggles and actionable diagnostics.

**Architecture:** Three new utilities: (1) `offset_to_line_col` converts byte offsets to 1-based line/column. (2) Levenshtein edit distance provides fuzzy matching for element names, CSS properties, and sensor paths. (3) A validation module checks templates and CSS against known-good lists and produces structured errors. The existing `ParseError` struct gains `line`, `column`, `severity`, and `suggestion` fields.

**Tech Stack:** Rust, no new dependencies.

**Testing notes:** All utilities are pure functions, fully unit-testable. Parser error output verified against known .omni inputs with intentional mistakes.

**Depends on:** Phase 9b complete (transitions, reactive classes, expression evaluator).

---

## File Map

```
host/
  src/
    omni/
      parser.rs              # Update ParseError struct, add offset_to_line_col, update all error sites
      validation.rs           # NEW: template + CSS + expression validation with suggestions
      mod.rs                  # Add pub mod validation;
```

---

### Task 1: Expand ParseError + Line/Column Helper

**Files:**
- Modify: `host/src/omni/parser.rs`

- [ ] **Step 1: Update ParseError struct**

Replace the current `ParseError`:

```rust
/// Severity level for parse diagnostics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// A parse error/warning with position and optional suggestion.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParseError {
    pub message: String,
    pub severity: Severity,
    pub line: usize,      // 1-based
    pub column: usize,    // 1-based
    pub suggestion: Option<String>,
}
```

- [ ] **Step 2: Add offset_to_line_col helper**

```rust
/// Convert a byte offset in a source string to 1-based (line, column).
pub fn offset_to_line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let last_newline = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let column = offset - last_newline + 1;
    (line, column)
}
```

- [ ] **Step 3: Update all ParseError construction sites**

Every place that creates a `ParseError` needs to:
1. Store the source string (pass it through to helper functions)
2. Call `offset_to_line_col(source, byte_offset)` to get line/column
3. Set severity (most are `Severity::Error`)
4. Set suggestion to `None` (validation module adds suggestions in Task 3)

The `parse_omni` function needs to accept and thread the source string. It already takes `source: &str`, so we just need to pass it down to `parse_widget`, `parse_template_children`, etc.

Create a helper to make error construction less verbose:

```rust
fn make_error(source: &str, offset: usize, message: String) -> ParseError {
    let (line, column) = offset_to_line_col(source, offset);
    ParseError {
        message,
        severity: Severity::Error,
        line,
        column,
        suggestion: None,
    }
}

fn make_warning(source: &str, offset: usize, message: String, suggestion: Option<String>) -> ParseError {
    let (line, column) = offset_to_line_col(source, offset);
    ParseError {
        message,
        severity: Severity::Warning,
        line,
        column,
        suggestion,
    }
}
```

Update ALL existing `ParseError { message, offset }` constructions to use `make_error(source, offset, message)`. This requires threading the `source` parameter through the internal parser functions. The functions that need it:
- `parse_widget` — add `source: &str` parameter
- `parse_template_children` — add `source: &str` parameter
- `parse_html_element` — add `source: &str` parameter
- `read_text_content` — add `source: &str` parameter
- `parse_config_block` — add `source: &str` parameter

- [ ] **Step 4: Update tests**

Existing tests that check `ParseError` fields need updating. For example:

```rust
#[test]
fn missing_widget_id_returns_error() {
    let source = r#"
        <widget name="Test" enabled="true">
            <template><div></div></template>
            <style></style>
        </widget>
    "#;

    let result = parse_omni(source);
    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors[0].severity, Severity::Error);
    assert!(errors[0].message.contains("missing required 'id'"));
    assert!(errors[0].line > 0);
    assert!(errors[0].column > 0);
}
```

Add a test for `offset_to_line_col`:

```rust
#[test]
fn offset_to_line_col_basic() {
    let source = "line1\nline2\nline3";
    assert_eq!(offset_to_line_col(source, 0), (1, 1));  // start of line1
    assert_eq!(offset_to_line_col(source, 5), (1, 6));  // newline at end of line1
    assert_eq!(offset_to_line_col(source, 6), (2, 1));  // start of line2
    assert_eq!(offset_to_line_col(source, 12), (3, 1)); // start of line3
}

#[test]
fn offset_to_line_col_beyond_end() {
    let source = "abc";
    assert_eq!(offset_to_line_col(source, 100), (1, 4)); // clamped
}
```

- [ ] **Step 5: Update WebSocket handlers**

In `ws_server.rs`, the `widget.parse` and `widget.apply` handlers serialize `ParseError` to JSON. Since we changed the struct (added fields, removed `offset`), the JSON output now includes `line`, `column`, `severity`, `suggestion` instead of `offset`. Update the serialization in the handlers:

The existing code:
```rust
json!({ "message": e.message, "offset": e.offset })
```
becomes:
```rust
json!({
    "message": e.message,
    "severity": e.severity,
    "line": e.line,
    "column": e.column,
    "suggestion": e.suggestion,
})
```

Or since `ParseError` derives `Serialize`, just use `serde_json::to_value(&e)`.

- [ ] **Step 6: Verify all tests pass**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add host/src/omni/parser.rs host/src/ws_server.rs
git commit -m "feat(host): expand ParseError with line/column, severity, suggestions"
```

---

### Task 2: Edit Distance + Suggestion Engine

**Files:**
- Create: `host/src/omni/validation.rs`
- Modify: `host/src/omni/mod.rs` (add `pub mod validation;`)

- [ ] **Step 1: Create host/src/omni/validation.rs**

```rust
//! Validation and suggestion engine for .omni files.
//!
//! Provides edit-distance-based fuzzy matching for element names,
//! CSS properties, and sensor paths.

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
pub fn validate_css_properties(
    css_source: &str,
    omni_source: &str,
    base_offset: usize,
) -> Vec<super::parser::ParseError> {
    use super::parser::{ParseError, Severity, offset_to_line_col};

    let mut warnings = Vec::new();

    // Simple property extraction: find "property:" patterns
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
                let (line, column) = offset_to_line_col(omni_source, prop_offset);
                warnings.push(ParseError {
                    message: format!("unsupported CSS property \"{}\"", prop_name),
                    severity: Severity::Warning,
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
pub fn validate_sensor_paths(
    template_text: &str,
    omni_source: &str,
    text_offset: usize,
) -> Vec<super::parser::ParseError> {
    use super::parser::{ParseError, Severity, offset_to_line_col};
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
                    let (line, column) = offset_to_line_col(omni_source, text_offset + start);
                    warnings.push(ParseError {
                        message: format!("unknown sensor path \"{}\"", path),
                        severity: Severity::Warning,
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
    fn suggest_element_typo() {
        assert_eq!(suggest_element("dvi"), Some("did you mean <div>?".to_string()));
        assert_eq!(suggest_element("sapn"), Some("did you mean <span>?".to_string()));
        assert_eq!(suggest_element("completely_wrong"), None); // too far
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
        let css = ".panel { colr: red; font-size: 14px; }";
        let omni = css; // for offset purposes
        let warnings = validate_css_properties(css, omni, 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("colr"));
        assert_eq!(warnings[0].suggestion, Some("did you mean \"color\"?".to_string()));
    }

    #[test]
    fn validate_css_ignores_custom_properties() {
        let css = ":root { --bg: red; --text: white; }";
        let warnings = validate_css_properties(css, css, 0);
        assert_eq!(warnings.len(), 0);
    }

    #[test]
    fn validate_sensor_path_unknown() {
        let text = "CPU: {cpu.usag}%";
        let warnings = validate_sensor_paths(text, text, 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].message.contains("cpu.usag"));
        assert!(warnings[0].suggestion.as_ref().unwrap().contains("cpu.usage"));
    }

    #[test]
    fn validate_sensor_path_valid() {
        let text = "CPU: {cpu.usage}%";
        let warnings = validate_sensor_paths(text, text, 0);
        assert_eq!(warnings.len(), 0);
    }
}
```

- [ ] **Step 2: Add module to mod.rs**

Add `pub mod validation;` to `host/src/omni/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host -- omni::validation`
Expected: 10+ tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/validation.rs host/src/omni/mod.rs
git commit -m "feat(host): add edit distance and suggestion engine for error diagnostics"
```

---

### Task 3: Integrate Validation into Parser

**Files:**
- Modify: `host/src/omni/parser.rs`

Wire the validation module into the parser to produce warnings alongside errors.

- [ ] **Step 1: Add element validation**

In `parse_html_element`, after extracting the tag name, check if it's a known element:

```rust
let tag = String::from_utf8_lossy(start.name().as_ref()).to_string();

// Validate element name
if !["div", "span"].contains(&tag.as_str()) {
    let suggestion = super::validation::suggest_element(&tag);
    // Don't fail — just warn. Unknown elements are rendered as containers.
    // Store warning to return alongside the parse result.
}
```

Since the parser currently returns `Result<OmniFile, Vec<ParseError>>` (either success OR errors), we need to change it to return BOTH the parsed file AND any warnings. Update the return type:

```rust
pub struct ParseResult {
    pub file: OmniFile,
    pub warnings: Vec<ParseError>,
}

pub fn parse_omni(source: &str) -> Result<ParseResult, Vec<ParseError>>
```

Or simpler: return `(OmniFile, Vec<ParseError>)` where the vec contains both errors and warnings. Errors prevent parsing (widget skipped), warnings are informational.

Actually, the simplest approach that maintains backward compatibility:

```rust
pub fn parse_omni(source: &str) -> Result<OmniFile, Vec<ParseError>>
```

Change to collect warnings alongside parsing. On success, attach warnings to the result. Update to:

```rust
pub fn parse_omni_with_diagnostics(source: &str) -> (Option<OmniFile>, Vec<ParseError>)
```

Where the vec contains errors AND warnings. If there are fatal errors, `OmniFile` is None. Warnings don't prevent parsing.

Keep the old `parse_omni` as a wrapper for backward compat:
```rust
pub fn parse_omni(source: &str) -> Result<OmniFile, Vec<ParseError>> {
    let (file, diagnostics) = parse_omni_with_diagnostics(source);
    let errors: Vec<ParseError> = diagnostics.iter()
        .filter(|d| d.severity == Severity::Error)
        .cloned()
        .collect();
    if errors.is_empty() {
        Ok(file.unwrap_or_else(OmniFile::empty))
    } else {
        Err(errors)
    }
}
```

- [ ] **Step 2: Add CSS validation after parsing each widget**

After parsing a widget's `<style>` block, validate its CSS properties:

```rust
let css_warnings = super::validation::validate_css_properties(
    &style_source, source, style_offset,
);
warnings.extend(css_warnings);
```

- [ ] **Step 3: Add sensor path validation**

When parsing text content in templates, validate sensor paths in `{...}` expressions:

```rust
// In parse_html_element or parse_template_children, after collecting text content
let path_warnings = super::validation::validate_sensor_paths(
    &text_content, source, text_offset,
);
warnings.extend(path_warnings);
```

- [ ] **Step 4: Update WebSocket handlers**

Update `widget.parse` and `widget.apply` in `ws_server.rs` to use `parse_omni_with_diagnostics` and return ALL diagnostics (errors + warnings):

```rust
"widget.parse" => {
    let source = msg.get("source").and_then(|v| v.as_str()).unwrap_or("");
    let (file, diagnostics) = crate::omni::parser::parse_omni_with_diagnostics(source);
    let diag_json: Vec<Value> = diagnostics.iter()
        .map(|d| serde_json::to_value(d).unwrap_or(json!(null)))
        .collect();
    Some(json!({
        "type": "widget.parsed",
        "file": file.map(|f| serde_json::to_value(&f).unwrap_or(json!(null))),
        "diagnostics": diag_json,
    }).to_string())
}
```

- [ ] **Step 5: Add CLI logging for warnings**

In `main.rs`, when loading the overlay, log warnings:

```rust
let (file, diagnostics) = omni::parser::parse_omni_with_diagnostics(&omni_source);
for diag in &diagnostics {
    match diag.severity {
        omni::parser::Severity::Error => error!(
            line = diag.line, col = diag.column,
            msg = %diag.message,
            suggestion = ?diag.suggestion,
            "parse error"
        ),
        omni::parser::Severity::Warning => warn!(
            line = diag.line, col = diag.column,
            msg = %diag.message,
            suggestion = ?diag.suggestion,
            "parse warning"
        ),
    }
}
```

- [ ] **Step 6: Verify all tests pass**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add host/src/omni/parser.rs host/src/ws_server.rs host/src/main.rs
git commit -m "feat(host): integrate validation into parser with element/CSS/sensor warnings"
```

---

### Task 4: Integration Test — Error Reporting

This is a manual integration test.

- [ ] **Step 1: Build**

```cmd
cargo build -p omni-host
```

- [ ] **Step 2: Test CLI error output**

Create an .omni file with intentional errors:

```xml
<widget id="test" name="Test" enabled="true">
  <template>
    <dvi class="panel">
      <span>{gpu.tamp}°C</span>
    </dvi>
  </template>
  <style>
    .panel { colr: white; font-sie: 14px; }
  </style>
</widget>
```

Start the host. Logs should show:
```
WARN  overlay.omni:3:5 — unknown element "dvi", did you mean <div>?
WARN  overlay.omni:4:13 — unknown sensor path "gpu.tamp", did you mean "gpu.temp"?
WARN  overlay.omni:8:14 — unsupported CSS property "colr", did you mean "color"?
WARN  overlay.omni:8:28 — unsupported CSS property "font-sie", did you mean "font-size"?
```

- [ ] **Step 3: Test WebSocket JSON output**

```javascript
const ws = new WebSocket('ws://localhost:9473');
ws.onmessage = (e) => console.log(JSON.parse(e.data));
ws.onopen = () => ws.send(JSON.stringify({
  type: 'widget.parse',
  source: '<widget id="test" name="Test" enabled="true"><template><dvi>{gpu.tamp}</dvi></template><style>.x { colr: red; }</style></widget>'
}));
```

Should return diagnostics with line, column, severity, and suggestions.

- [ ] **Step 4: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during Phase 9a-4 integration test"
```

---

## Phase 9a-4 Complete — Summary

At this point you have:

1. **Structured ParseError** — line, column (1-based), severity (Error/Warning), message, suggestion
2. **offset_to_line_col** — byte offset → line/column conversion
3. **Edit distance** — Levenshtein algorithm for fuzzy matching
4. **Element suggestions** — "did you mean `<div>`?" for unknown tags
5. **CSS property suggestions** — "did you mean `color`?" for typos
6. **Sensor path suggestions** — "did you mean `gpu.temp`?" for unknown paths
7. **CLI diagnostics** — Rust-compiler-style warnings in terminal
8. **JSON diagnostics** — structured errors for Monaco integration via WebSocket

**Next:** Phase 10 adds hot-reload and preview window.
