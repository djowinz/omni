//! CSS transition engine: parses transition properties, tracks active transitions,
//! and interpolates property values over time with easing functions.

use std::collections::HashMap;
use std::time::Instant;

use super::easing::EasingFunction;

/// A parsed transition rule for a single property.
#[derive(Debug, Clone)]
pub struct TransitionRule {
    pub property: String,
    pub duration_ms: f64,
    pub delay_ms: f64,
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
    /// Key: (widget_id, element_index) -> HashMap<property, value>
    previous_values: HashMap<(String, usize), HashMap<String, String>>,
}

impl TransitionManager {
    pub fn new() -> Self {
        Self {
            active: HashMap::new(),
            previous_values: HashMap::new(),
        }
    }

    /// Parse a CSS `transition` property value into rules.
    /// e.g., "width 0.3s ease, background 0.3s ease-in-out 0.1s"
    pub fn parse_transition(value: &str) -> Vec<TransitionRule> {
        let mut rules = Vec::new();
        for segment in value.split(',') {
            let segment = segment.trim();
            if segment.is_empty() {
                continue;
            }
            if let Some(rule) = parse_single_transition(segment) {
                rules.push(rule);
            }
        }
        rules
    }

    /// Update transitions for an element. Call once per element per frame.
    /// Uses `Instant::now()` for timing.
    pub fn update(
        &mut self,
        widget_id: &str,
        element_idx: usize,
        transition_rules: &[TransitionRule],
        current_values: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        self.update_at(widget_id, element_idx, transition_rules, current_values, Instant::now())
    }

    /// Update transitions with an explicit timestamp (for testability).
    pub fn update_at(
        &mut self,
        widget_id: &str,
        element_idx: usize,
        transition_rules: &[TransitionRule],
        current_values: &HashMap<String, String>,
        now: Instant,
    ) -> HashMap<String, String> {
        let key = (widget_id.to_string(), element_idx);

        // 1. Get previous values for this element
        let previous = self.previous_values.get(&key).cloned().unwrap_or_default();

        // 2. For each property in current_values, detect changes and start transitions
        for (prop, current_val) in current_values {
            if let Some(prev_val) = previous.get(prop) {
                if prev_val != current_val {
                    // Value changed — check if a transition rule matches
                    if let Some(rule) = find_matching_rule(transition_rules, prop) {
                        let active_key = (widget_id.to_string(), element_idx, prop.clone());
                        // If there's already an active transition for this property,
                        // use its current interpolated value as the new from_value
                        let from_value = if let Some(existing) = self.active.get(&active_key) {
                            let elapsed_ms = now.duration_since(existing.start_time).as_secs_f64() * 1000.0;
                            if elapsed_ms < existing.delay_ms {
                                existing.from_value.clone()
                            } else {
                                let t_raw = (elapsed_ms - existing.delay_ms) / existing.duration_ms;
                                let t = t_raw.clamp(0.0, 1.0);
                                let eased = existing.easing.apply(t);
                                interpolate_value(&existing.from_value, &existing.to_value, eased)
                            }
                        } else {
                            prev_val.clone()
                        };

                        self.active.insert(active_key, ActiveTransition {
                            property: prop.clone(),
                            from_value,
                            to_value: current_val.clone(),
                            start_time: now,
                            duration_ms: rule.duration_ms,
                            delay_ms: rule.delay_ms,
                            easing: rule.easing.clone(),
                        });
                    }
                }
            }
        }

        // 3. Evaluate active transitions and build overrides
        let mut overrides = HashMap::new();
        let mut completed = Vec::new();

        for (akey, transition) in &self.active {
            if akey.0 != widget_id || akey.1 != element_idx {
                continue;
            }

            let elapsed_ms = now.duration_since(transition.start_time).as_secs_f64() * 1000.0;

            if elapsed_ms < transition.delay_ms {
                // Still in delay period — show from_value
                overrides.insert(transition.property.clone(), transition.from_value.clone());
            } else if elapsed_ms >= transition.delay_ms + transition.duration_ms {
                // Transition complete
                overrides.insert(transition.property.clone(), transition.to_value.clone());
                completed.push(akey.clone());
            } else {
                // In progress — interpolate
                let t_raw = (elapsed_ms - transition.delay_ms) / transition.duration_ms;
                let t = t_raw.clamp(0.0, 1.0);
                let eased = transition.easing.apply(t);
                let interpolated = interpolate_value(&transition.from_value, &transition.to_value, eased);
                overrides.insert(transition.property.clone(), interpolated);
            }
        }

        // 4. Remove completed transitions
        for key in completed {
            self.active.remove(&key);
        }

        // 5. Store current_values as new previous
        self.previous_values.insert(key, current_values.clone());

        overrides
    }
}

/// Find a matching transition rule for the given property name.
/// Matches by exact name or the "all" keyword.
fn find_matching_rule<'a>(rules: &'a [TransitionRule], property: &str) -> Option<&'a TransitionRule> {
    // First try exact match
    if let Some(rule) = rules.iter().find(|r| r.property == property) {
        return Some(rule);
    }
    // Then try "all"
    rules.iter().find(|r| r.property == "all")
}

/// Parse a single transition segment like "width 0.3s ease 0.1s".
/// Format: <property> <duration> [<easing>] [<delay>]
fn parse_single_transition(segment: &str) -> Option<TransitionRule> {
    let parts: Vec<&str> = segment.split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    let property = parts[0].to_string();
    let mut duration_ms = 0.0;
    let mut delay_ms = 0.0;
    let mut easing = EasingFunction::Ease;
    let mut found_duration = false;

    for &part in &parts[1..] {
        if let Some(ms) = parse_time(part) {
            if !found_duration {
                duration_ms = ms;
                found_duration = true;
            } else {
                delay_ms = ms;
            }
        } else {
            easing = EasingFunction::parse(part);
        }
    }

    Some(TransitionRule {
        property,
        duration_ms,
        delay_ms,
        easing,
    })
}

/// Parse a CSS time value like "0.3s" or "300ms" into milliseconds.
fn parse_time(s: &str) -> Option<f64> {
    if let Some(val) = s.strip_suffix("ms") {
        val.parse::<f64>().ok()
    } else if let Some(val) = s.strip_suffix('s') {
        val.parse::<f64>().ok().map(|v| v * 1000.0)
    } else {
        None
    }
}

/// Interpolate between two CSS values at progress t (0.0 to 1.0).
pub fn interpolate_value(from: &str, to: &str, t: f64) -> String {
    // Try color interpolation (rgba)
    if let (Some(from_c), Some(to_c)) = (parse_rgba(from), parse_rgba(to)) {
        let r = lerp(from_c.0, to_c.0, t).round() as i64;
        let g = lerp(from_c.1, to_c.1, t).round() as i64;
        let b = lerp(from_c.2, to_c.2, t).round() as i64;
        let a = lerp(from_c.3, to_c.3, t);
        // Clamp channels
        let r = r.clamp(0, 255);
        let g = g.clamp(0, 255);
        let b = b.clamp(0, 255);
        let a = a.clamp(0.0, 1.0);
        // Format alpha: use integer if whole, otherwise limited decimals
        if (a - a.round()).abs() < 1e-6 {
            return format!("rgba({},{},{},{})", r, g, b, a.round() as i64);
        }
        return format!("rgba({},{},{},{})", r, g, b, format_float(a));
    }

    // Try numeric with unit (e.g., "60px", "12rem", "45deg")
    if let (Some((from_n, from_u)), Some((to_n, to_u))) = (parse_numeric_unit(from), parse_numeric_unit(to)) {
        if from_u == to_u {
            let val = lerp(from_n, to_n, t);
            // Format cleanly: avoid unnecessary decimals
            return format!("{}{}", format_number(val), from_u);
        }
    }

    // Try plain numbers
    if let (Ok(from_n), Ok(to_n)) = (from.parse::<f64>(), to.parse::<f64>()) {
        let val = lerp(from_n, to_n, t);
        return format_float(val);
    }

    // Non-interpolatable: snap to target at t >= 1.0
    if t >= 1.0 {
        to.to_string()
    } else {
        from.to_string()
    }
}

/// Parse an rgba(...) color string into (r, g, b, a) components.
/// Parse a color string (rgba, rgb, hex) into (r, g, b, a) components.
/// Supports: rgba(r,g,b,a), rgb(r,g,b), #RRGGBB, #RRGGBBAA, #RGB, #RGBA
fn parse_rgba(s: &str) -> Option<(f64, f64, f64, f64)> {
    let s = s.trim();

    // Hex colors
    if s.starts_with('#') {
        return parse_hex_to_rgba(s);
    }

    // rgba(...) or rgb(...)
    let inner = if let Some(rest) = s.strip_prefix("rgba(") {
        rest.strip_suffix(')')?
    } else if let Some(rest) = s.strip_prefix("rgb(") {
        rest.strip_suffix(')')?
    } else {
        // Named colors
        return match s {
            "white" => Some((255.0, 255.0, 255.0, 1.0)),
            "black" => Some((0.0, 0.0, 0.0, 1.0)),
            "red" => Some((255.0, 0.0, 0.0, 1.0)),
            "green" => Some((0.0, 128.0, 0.0, 1.0)),
            "blue" => Some((0.0, 0.0, 255.0, 1.0)),
            "transparent" => Some((0.0, 0.0, 0.0, 0.0)),
            _ => None,
        };
    };

    let parts: Vec<&str> = inner.split(',').collect();
    if parts.len() == 4 {
        let r = parts[0].trim().parse::<f64>().ok()?;
        let g = parts[1].trim().parse::<f64>().ok()?;
        let b = parts[2].trim().parse::<f64>().ok()?;
        let a = parts[3].trim().parse::<f64>().ok()?;
        Some((r, g, b, a))
    } else if parts.len() == 3 {
        let r = parts[0].trim().parse::<f64>().ok()?;
        let g = parts[1].trim().parse::<f64>().ok()?;
        let b = parts[2].trim().parse::<f64>().ok()?;
        Some((r, g, b, 1.0))
    } else {
        None
    }
}

/// Parse hex color to (r, g, b, a) with values 0-255 for rgb and 0-1 for alpha.
fn parse_hex_to_rgba(hex: &str) -> Option<(f64, f64, f64, f64)> {
    let hex = hex.strip_prefix('#')?;
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()? as f64;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()? as f64;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()? as f64;
            Some((r, g, b, 1.0))
        }
        4 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()? as f64;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()? as f64;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()? as f64;
            let a = u8::from_str_radix(&hex[3..4].repeat(2), 16).ok()? as f64 / 255.0;
            Some((r, g, b, a))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f64;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f64;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f64;
            Some((r, g, b, 1.0))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()? as f64;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()? as f64;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()? as f64;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()? as f64 / 255.0;
            Some((r, g, b, a))
        }
        _ => None,
    }
}

/// Parse a numeric value with a CSS unit, e.g., "60px" -> (60.0, "px").
fn parse_numeric_unit(s: &str) -> Option<(f64, String)> {
    let s = s.trim();
    // Find where the digits end and the unit begins
    let num_end = s
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(s.len());

    if num_end == 0 || num_end == s.len() {
        // No unit found or no number found
        return None;
    }

    let num_str = &s[..num_end];
    let unit = &s[num_end..];
    let num = num_str.parse::<f64>().ok()?;
    Some((num, unit.to_string()))
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// Format a float without unnecessary trailing zeros, but keep at least one decimal
/// if there are decimals.
fn format_float(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        // Use a reasonable number of decimal places
        let s = format!("{:.4}", v);
        // Trim trailing zeros after the decimal point
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }
}

/// Format a number for CSS output — integer if whole, otherwise trimmed float.
fn format_number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{:.2}", v);
        let s = s.trim_end_matches('0');
        let s = s.trim_end_matches('.');
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // --- Parsing tests ---

    #[test]
    fn parse_single_rule() {
        let rules = TransitionManager::parse_transition("width 0.3s ease");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].property, "width");
        assert!((rules[0].duration_ms - 300.0).abs() < 0.01);
        assert!((rules[0].delay_ms - 0.0).abs() < 0.01);
        assert!(matches!(rules[0].easing, EasingFunction::Ease));
    }

    #[test]
    fn parse_two_rules() {
        let rules = TransitionManager::parse_transition("width 0.3s ease, opacity 0.5s linear");
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].property, "width");
        assert!((rules[0].duration_ms - 300.0).abs() < 0.01);
        assert_eq!(rules[1].property, "opacity");
        assert!((rules[1].duration_ms - 500.0).abs() < 0.01);
        assert!(matches!(rules[1].easing, EasingFunction::Linear));
    }

    #[test]
    fn parse_all_property() {
        let rules = TransitionManager::parse_transition("all 0.3s ease-in-out");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].property, "all");
        assert!(matches!(rules[0].easing, EasingFunction::EaseInOut));
    }

    #[test]
    fn parse_with_delay() {
        let rules = TransitionManager::parse_transition("width 0.3s ease 0.1s");
        assert_eq!(rules.len(), 1);
        assert!((rules[0].duration_ms - 300.0).abs() < 0.01);
        assert!((rules[0].delay_ms - 100.0).abs() < 0.01);
    }

    #[test]
    fn parse_ms_duration() {
        let rules = TransitionManager::parse_transition("opacity 200ms linear");
        assert_eq!(rules.len(), 1);
        assert!((rules[0].duration_ms - 200.0).abs() < 0.01);
    }

    #[test]
    fn parse_empty_string() {
        let rules = TransitionManager::parse_transition("");
        assert!(rules.is_empty());
    }

    // --- Interpolation tests ---

    #[test]
    fn interpolate_numeric_px() {
        let result = interpolate_value("60px", "200px", 0.5);
        assert_eq!(result, "130px");
    }

    #[test]
    fn interpolate_numeric_px_at_zero() {
        let result = interpolate_value("60px", "200px", 0.0);
        assert_eq!(result, "60px");
    }

    #[test]
    fn interpolate_numeric_px_at_one() {
        let result = interpolate_value("60px", "200px", 1.0);
        assert_eq!(result, "200px");
    }

    #[test]
    fn interpolate_rgba_color() {
        let result = interpolate_value("rgba(0,0,0,1)", "rgba(255,0,0,1)", 0.5);
        // Should be approximately rgba(128,0,0,1) — rounding may give 127 or 128
        assert!(result.starts_with("rgba("));
        let inner = result.strip_prefix("rgba(").unwrap().strip_suffix(')').unwrap();
        let parts: Vec<f64> = inner.split(',').map(|p| p.trim().parse().unwrap()).collect();
        assert!((parts[0] - 128.0).abs() <= 1.0, "red channel: {}", parts[0]);
        assert!((parts[1] - 0.0).abs() < 0.01);
        assert!((parts[2] - 0.0).abs() < 0.01);
        assert!((parts[3] - 1.0).abs() < 0.01);
    }

    #[test]
    fn interpolate_rgba_alpha() {
        let result = interpolate_value("rgba(20,30,40,0.5)", "rgba(20,30,40,1)", 0.5);
        let inner = result.strip_prefix("rgba(").unwrap().strip_suffix(')').unwrap();
        let parts: Vec<f64> = inner.split(',').map(|p| p.trim().parse().unwrap()).collect();
        assert!((parts[3] - 0.75).abs() < 0.01, "alpha: {}", parts[3]);
    }

    #[test]
    fn interpolate_plain_number() {
        let result = interpolate_value("0", "1", 0.5);
        assert_eq!(result, "0.5");
    }

    #[test]
    fn interpolate_non_interpolatable_before_end() {
        let result = interpolate_value("block", "none", 0.5);
        assert_eq!(result, "block");
    }

    #[test]
    fn interpolate_non_interpolatable_at_end() {
        let result = interpolate_value("block", "none", 1.0);
        assert_eq!(result, "none");
    }

    // --- TransitionManager update tests ---

    #[test]
    fn update_detects_change_and_starts_transition() {
        let mut tm = TransitionManager::new();
        let rules = TransitionManager::parse_transition("width 0.3s linear");
        let now = Instant::now();

        // First frame: establish previous values
        let mut vals1 = HashMap::new();
        vals1.insert("width".to_string(), "60px".to_string());
        let overrides = tm.update_at("w1", 0, &rules, &vals1, now);
        assert!(overrides.is_empty(), "no transition on first frame");

        // Second frame: change value
        let mut vals2 = HashMap::new();
        vals2.insert("width".to_string(), "200px".to_string());
        let later = now + Duration::from_millis(16);
        let overrides = tm.update_at("w1", 0, &rules, &vals2, later);
        assert!(overrides.contains_key("width"), "should start transition for width");
        // The override should be the from_value since almost no time has passed
        // (only 0ms into the transition since it just started)
    }

    #[test]
    fn update_returns_interpolated_midtransition() {
        let mut tm = TransitionManager::new();
        let rules = TransitionManager::parse_transition("width 1s linear");
        let now = Instant::now();

        // Frame 1: establish
        let mut vals = HashMap::new();
        vals.insert("width".to_string(), "0px".to_string());
        tm.update_at("w1", 0, &rules, &vals, now);

        // Frame 2: trigger change
        let mut vals2 = HashMap::new();
        vals2.insert("width".to_string(), "100px".to_string());
        let t1 = now + Duration::from_millis(16);
        tm.update_at("w1", 0, &rules, &vals2, t1);

        // Frame 3: 500ms into transition (50%)
        let t2 = t1 + Duration::from_millis(500);
        let overrides = tm.update_at("w1", 0, &rules, &vals2, t2);
        let width = overrides.get("width").expect("should have width override");
        assert_eq!(width, "50px", "at 50% linear, should be 50px, got {}", width);
    }

    #[test]
    fn completed_transition_returns_final_value() {
        let mut tm = TransitionManager::new();
        let rules = TransitionManager::parse_transition("width 0.3s linear");
        let now = Instant::now();

        // Frame 1: establish
        let mut vals = HashMap::new();
        vals.insert("width".to_string(), "0px".to_string());
        tm.update_at("w1", 0, &rules, &vals, now);

        // Frame 2: trigger change
        let mut vals2 = HashMap::new();
        vals2.insert("width".to_string(), "100px".to_string());
        let t1 = now + Duration::from_millis(16);
        tm.update_at("w1", 0, &rules, &vals2, t1);

        // Frame 3: well past duration (1 second later)
        let t2 = t1 + Duration::from_millis(1000);
        let overrides = tm.update_at("w1", 0, &rules, &vals2, t2);
        let width = overrides.get("width").expect("should have final width");
        assert_eq!(width, "100px");
    }

    #[test]
    fn update_respects_delay() {
        let mut tm = TransitionManager::new();
        let rules = TransitionManager::parse_transition("width 0.3s linear 0.2s");
        let now = Instant::now();

        // Frame 1: establish
        let mut vals = HashMap::new();
        vals.insert("width".to_string(), "0px".to_string());
        tm.update_at("w1", 0, &rules, &vals, now);

        // Frame 2: trigger change
        let mut vals2 = HashMap::new();
        vals2.insert("width".to_string(), "100px".to_string());
        let t1 = now + Duration::from_millis(16);
        tm.update_at("w1", 0, &rules, &vals2, t1);

        // Frame 3: 100ms in — still in delay, should show from_value
        let t2 = t1 + Duration::from_millis(100);
        let overrides = tm.update_at("w1", 0, &rules, &vals2, t2);
        let width = overrides.get("width").expect("should have width");
        assert_eq!(width, "0px", "during delay, should show from_value");
    }

    #[test]
    fn no_transition_without_matching_rule() {
        let mut tm = TransitionManager::new();
        // Only transition "opacity", not "width"
        let rules = TransitionManager::parse_transition("opacity 0.3s linear");
        let now = Instant::now();

        let mut vals = HashMap::new();
        vals.insert("width".to_string(), "60px".to_string());
        tm.update_at("w1", 0, &rules, &vals, now);

        let mut vals2 = HashMap::new();
        vals2.insert("width".to_string(), "200px".to_string());
        let t1 = now + Duration::from_millis(16);
        let overrides = tm.update_at("w1", 0, &rules, &vals2, t1);
        assert!(!overrides.contains_key("width"), "width has no matching rule");
    }

    #[test]
    fn all_rule_matches_any_property() {
        let mut tm = TransitionManager::new();
        let rules = TransitionManager::parse_transition("all 0.5s linear");
        let now = Instant::now();

        let mut vals = HashMap::new();
        vals.insert("width".to_string(), "10px".to_string());
        vals.insert("opacity".to_string(), "0".to_string());
        tm.update_at("w1", 0, &rules, &vals, now);

        let mut vals2 = HashMap::new();
        vals2.insert("width".to_string(), "100px".to_string());
        vals2.insert("opacity".to_string(), "1".to_string());
        let t1 = now + Duration::from_millis(16);
        let overrides = tm.update_at("w1", 0, &rules, &vals2, t1);
        assert!(overrides.contains_key("width"), "all should match width");
        assert!(overrides.contains_key("opacity"), "all should match opacity");
    }
}
