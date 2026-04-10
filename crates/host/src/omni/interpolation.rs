//! Template expression parser and evaluator for `{...}` interpolation.
//!
//! Three expression shapes are supported inside `{...}`:
//! 1. Bare sensor path: `cpu.usage`
//! 2. Sensor with precision: `cpu.usage(2)` (legacy shorthand)
//! 3. Function call: `format_value(cpu.usage, %)`
//!
//! Function names are resolved against a registry. If an identifier is
//! not a known function, the parser falls back to the sensor-path form.
//! This preserves backward compatibility with `{cpu.usage(2)}`.

use std::collections::HashMap;

use omni_shared::SensorSnapshot;

use super::history::SensorHistory;
use super::sensor_map;
use super::units::Unit;

/// Evaluation context passed to interpolate/evaluate functions.
pub struct EvalCtx<'a> {
    pub snapshot: &'a SensorSnapshot,
    pub history: &'a SensorHistory,
    pub hwinfo_values: &'a HashMap<String, f64>,
    pub hwinfo_units: &'a HashMap<String, String>,
}

/// A single parsed expression from inside `{...}`.
#[derive(Debug, Clone)]
pub enum Expression {
    /// `cpu.usage` or `cpu.usage(2)` — path with optional precision
    SensorPath {
        path: String,
        precision: Option<usize>,
    },
    /// `format_value(cpu.usage, %)` — named function with argument list
    FunctionCall {
        name: String,
        args: Vec<Argument>,
    },
    /// Passthrough for malformed or unknown content
    Literal(String),
}

/// A single argument within a function call.
#[derive(Debug, Clone)]
pub enum Argument {
    /// Numeric literal: `200`, `-1.5`
    Number(f64),
    /// Sensor path: `cpu.usage`, `network.bytes_per_sec`
    SensorPath(String),
    /// Unit identifier: `bytes/s`, `%`, `Hz`
    Unit(Unit),
    /// Unknown identifier (fallback)
    Identifier(String),
}

/// Set of known function names. Unknown identifiers with parentheses fall
/// back to sensor-with-precision parsing. Populated as helper functions are
/// added in later tasks.
fn is_known_function(name: &str) -> bool {
    matches!(
        name,
        "format_value"
            | "buffer_min"
            | "buffer_max"
            | "buffer_avg"
            | "nice_min"
            | "nice_max"
            | "nice_tick"
            | "chart_polyline"
            | "chart_path"
            | "bar_height"
            | "bar_y"
            | "circumference"
            | "ratio_dashoffset"
    )
}

/// Parse a single expression (the content inside `{...}`). Returns a
/// `Literal` for malformed input.
pub fn parse_expression(input: &str) -> Expression {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Expression::Literal(String::new());
    }

    // Look for a `(` that starts a function call or precision.
    if let Some(paren_start) = trimmed.find('(') {
        let name = trimmed[..paren_start].trim();
        if !trimmed.ends_with(')') {
            return Expression::Literal(input.to_string());
        }
        let args_str = &trimmed[paren_start + 1..trimmed.len() - 1];

        // Disambiguate function call vs. sensor(precision).
        if is_known_function(name) {
            let args = parse_arguments(args_str);
            return Expression::FunctionCall {
                name: name.to_string(),
                args,
            };
        }
        // Not a known function — try sensor(precision)
        if let Ok(prec) = args_str.trim().parse::<usize>() {
            return Expression::SensorPath {
                path: name.to_string(),
                precision: Some(prec),
            };
        }
        // Unknown and not a numeric precision — emit as literal
        return Expression::Literal(input.to_string());
    }

    // No parens — bare sensor path
    Expression::SensorPath {
        path: trimmed.to_string(),
        precision: None,
    }
}

/// Parse a comma-separated argument list from the content inside `(...)`.
fn parse_arguments(input: &str) -> Vec<Argument> {
    let mut args = Vec::new();
    for raw in input.split(',') {
        let a = raw.trim();
        if a.is_empty() {
            continue;
        }
        // Try numeric literal first
        if let Ok(n) = a.parse::<f64>() {
            args.push(Argument::Number(n));
            continue;
        }
        // Try unit identifier
        if let Some(u) = Unit::parse(a) {
            args.push(Argument::Unit(u));
            continue;
        }
        // Sensor path (contains a dot) or bare identifier
        if a.contains('.') {
            args.push(Argument::SensorPath(a.to_string()));
        } else {
            args.push(Argument::Identifier(a.to_string()));
        }
    }
    args
}

/// Evaluate a single expression to a display string.
pub fn evaluate(expr: &Expression, ctx: &EvalCtx) -> String {
    match expr {
        Expression::SensorPath { path, precision } => {
            sensor_map::get_sensor_value_with_hwinfo(
                path,
                ctx.snapshot,
                ctx.hwinfo_values,
                ctx.hwinfo_units,
                *precision,
            )
        }
        Expression::FunctionCall { name, args } => {
            // Dispatch to helper functions. Populated in Tasks 9 and 10.
            evaluate_function(name, args, ctx)
                .unwrap_or_else(|| format!("{{{}()}}", name))
        }
        Expression::Literal(s) => format!("{{{}}}", s),
    }
}

/// Dispatch to helper function implementations. Returns None for unknown
/// functions. Populated in later tasks.
#[allow(unused_variables, clippy::match_single_binding)]
fn evaluate_function(name: &str, args: &[Argument], ctx: &EvalCtx) -> Option<String> {
    // Stub — real implementations arrive in Tasks 9 and 10.
    match name {
        _ => None,
    }
}

/// Scan `input` for `{...}` expressions and replace each with its
/// evaluated result.
#[allow(dead_code)]
pub fn interpolate(input: &str, ctx: &EvalCtx) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut body = String::new();
            let mut found_close = false;
            for inner in chars.by_ref() {
                if inner == '}' {
                    found_close = true;
                    break;
                }
                body.push(inner);
            }

            if !found_close {
                // Malformed — emit verbatim
                result.push('{');
                result.push_str(&body);
                continue;
            }
            if body.is_empty() {
                result.push('{');
                result.push('}');
                continue;
            }

            let expr = parse_expression(&body);
            match expr {
                Expression::Literal(s) => {
                    // Bad parse — emit verbatim
                    result.push('{');
                    result.push_str(&s);
                    result.push('}');
                }
                _ => {
                    result.push_str(&evaluate(&expr, ctx));
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::omni::history::SensorHistory;
    use omni_shared::SensorSnapshot;
    use std::collections::HashMap;

    fn make_ctx<'a>(
        snapshot: &'a SensorSnapshot,
        history: &'a SensorHistory,
        hwinfo_values: &'a HashMap<String, f64>,
        hwinfo_units: &'a HashMap<String, String>,
    ) -> EvalCtx<'a> {
        EvalCtx {
            snapshot,
            history,
            hwinfo_values,
            hwinfo_units,
        }
    }

    #[test]
    fn interpolate_bare_sensor_path() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 42.0;
        let history = SensorHistory::new();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("CPU: {cpu.usage}%", &ctx);
        assert_eq!(result, "CPU: 42%");
    }

    #[test]
    fn interpolate_sensor_with_precision() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.gpu.temp_c = 71.5;
        let history = SensorHistory::new();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{gpu.temp(1)}", &ctx);
        assert_eq!(result, "71.5");
    }

    #[test]
    fn interpolate_multiple_expressions() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 42.0;
        snapshot.gpu.temp_c = 71.0;
        let history = SensorHistory::new();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{cpu.usage}% | {gpu.temp}°C", &ctx);
        assert_eq!(result, "42% | 71°C");
    }

    #[test]
    fn interpolate_malformed_expression_passthrough() {
        let snapshot = SensorSnapshot::default();
        let history = SensorHistory::new();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("text {unterminated", &ctx);
        assert_eq!(result, "text {unterminated");
    }

    #[test]
    fn parse_function_call_shape() {
        let expr = parse_expression("format_value(cpu.usage, %)");
        match expr {
            Expression::FunctionCall { name, args } => {
                assert_eq!(name, "format_value");
                assert_eq!(args.len(), 2);
            }
            other => panic!("Expected FunctionCall, got {:?}", other),
        }
    }

    #[test]
    fn parse_bare_sensor_path() {
        let expr = parse_expression("cpu.usage");
        match expr {
            Expression::SensorPath { path, precision } => {
                assert_eq!(path, "cpu.usage");
                assert_eq!(precision, None);
            }
            other => panic!("Expected SensorPath, got {:?}", other),
        }
    }

    #[test]
    fn parse_sensor_path_with_precision() {
        let expr = parse_expression("cpu.usage(2)");
        match expr {
            Expression::SensorPath { path, precision } => {
                assert_eq!(path, "cpu.usage");
                assert_eq!(precision, Some(2));
            }
            other => panic!("Expected SensorPath, got {:?}", other),
        }
    }
}
