//! Icon font class→codepoint mapping, parsed from feather.css.
//!
//! The resolver uses this to convert `<span class="icon icon-cpu"></span>`
//! into a text widget with the correct Unicode glyph and font-family "feather".

use std::collections::HashMap;
use std::path::Path;

/// Parsed icon font mapping: class name (e.g., "icon-cpu") → Unicode char.
pub struct IconFontMap {
    /// Maps icon class name (without the "icon-" prefix) to Unicode character.
    icons: HashMap<String, char>,
}

impl IconFontMap {
    /// Parse a feather-font CSS file to extract icon class→codepoint mappings.
    /// Looks for patterns like: `.icon-cpu:before { content: "\e85e"; }`
    pub fn from_css_file(path: &Path) -> Self {
        let mut icons = HashMap::new();

        let css = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => {
                tracing::warn!(path = %path.display(), "Failed to read icon font CSS");
                return Self { icons };
            }
        };

        Self::parse_css(&css, &mut icons);
        tracing::info!(count = icons.len(), "Loaded icon font mappings");

        Self { icons }
    }

    /// Parse CSS content for icon mappings.
    fn parse_css(css: &str, icons: &mut HashMap<String, char>) {
        // Match: .icon-{name}:before { content: "\e{hex}"; }
        let re_pattern = r#"\.icon-([a-zA-Z0-9_-]+):before\s*\{\s*content:\s*"\\([0-9a-fA-F]+)";"#;
        let re = regex_lite::Regex::new(re_pattern).unwrap();

        for cap in re.captures_iter(css) {
            let name = cap.get(1).unwrap().as_str().to_string();
            let hex = cap.get(2).unwrap().as_str();
            if let Ok(codepoint) = u32::from_str_radix(hex, 16) {
                if let Some(ch) = char::from_u32(codepoint) {
                    icons.insert(name, ch);
                }
            }
        }
    }

    /// Look up an icon by class name (e.g., "cpu" from "icon-cpu").
    pub fn get(&self, name: &str) -> Option<char> {
        self.icons.get(name).copied()
    }

    /// Check if a list of CSS classes contains an icon reference.
    /// Returns the icon character and removes the icon classes from the list.
    pub fn resolve_icon_classes(&self, classes: &[String]) -> Option<char> {
        // Look for the "icon-{name}" pattern
        for class in classes {
            if let Some(name) = class.strip_prefix("icon-") {
                if let Some(ch) = self.get(name) {
                    return Some(ch);
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_feather_css_pattern() {
        let css = r#"
            .icon-cpu:before { content: "\e954"; }
            .icon-activity:before { content: "\e922"; }
            .icon-zap:before { content: "\ea17"; }
        "#;
        let mut icons = HashMap::new();
        IconFontMap::parse_css(css, &mut icons);

        assert_eq!(icons.len(), 3);
        assert_eq!(icons.get("cpu"), Some(&'\u{e954}'));
        assert_eq!(icons.get("activity"), Some(&'\u{e922}'));
        assert_eq!(icons.get("zap"), Some(&'\u{ea17}'));
    }

    #[test]
    fn resolve_icon_from_classes() {
        let css = r#".icon-cpu:before { content: "\e954"; }"#;
        let mut icons = HashMap::new();
        IconFontMap::parse_css(css, &mut icons);
        let map = IconFontMap { icons };

        let classes = vec!["icon".to_string(), "icon-cpu".to_string()];
        assert_eq!(map.resolve_icon_classes(&classes), Some('\u{e954}'));

        let no_icon = vec!["panel".to_string()];
        assert_eq!(map.resolve_icon_classes(&no_icon), None);
    }

    #[test]
    fn parse_multiline_css_format() {
        // The actual feather.css uses multi-line format
        let css = r#"
.icon-cpu:before {
    content: "\e954";
}
.icon-thermometer:before {
    content: "\e9c6";
}
"#;
        let mut icons = HashMap::new();
        IconFontMap::parse_css(css, &mut icons);

        assert_eq!(icons.len(), 2, "Should parse multi-line CSS entries, got: {:?}", icons);
        assert_eq!(icons.get("cpu"), Some(&'\u{e954}'));
        assert_eq!(icons.get("thermometer"), Some(&'\u{e9c6}'));
    }
}
