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

/// Dispatch to helper function implementations. Returns None for unknown functions.
fn evaluate_function(name: &str, args: &[Argument], ctx: &EvalCtx) -> Option<String> {
    match name {
        "format_value" => eval_format_value(args, ctx),
        "buffer_min" => eval_buffer_stat(args, ctx, BufferStat::Min),
        "buffer_max" => eval_buffer_stat(args, ctx, BufferStat::Max),
        "buffer_avg" => eval_buffer_stat(args, ctx, BufferStat::Avg),
        "nice_min" => eval_nice_bound(args, ctx, NiceBound::Min),
        "nice_max" => eval_nice_bound(args, ctx, NiceBound::Max),
        "nice_tick" => eval_nice_tick(args, ctx),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum BufferStat {
    Min,
    Max,
    Avg,
}

#[derive(Clone, Copy)]
enum NiceBound {
    Min,
    Max,
}

fn arg_sensor_path<'a>(args: &'a [Argument], idx: usize) -> Option<&'a str> {
    match args.get(idx)? {
        Argument::SensorPath(s) | Argument::Identifier(s) => Some(s.as_str()),
        _ => None,
    }
}

fn arg_unit(args: &[Argument], idx: usize) -> Option<Unit> {
    match args.get(idx)? {
        Argument::Unit(u) => Some(*u),
        _ => None,
    }
}

fn arg_number(args: &[Argument], idx: usize) -> Option<f64> {
    match args.get(idx)? {
        Argument::Number(n) => Some(*n),
        _ => None,
    }
}

/// `format_value(sensor, unit)` or `format_value(number, unit)`
fn eval_format_value(args: &[Argument], ctx: &EvalCtx) -> Option<String> {
    let unit = arg_unit(args, 1)?;
    let value = match args.first()? {
        Argument::SensorPath(s) | Argument::Identifier(s) => {
            sensor_map::get_sensor_value_f64(s, ctx.snapshot, ctx.hwinfo_values)?
        }
        Argument::Number(n) => *n,
        _ => return None,
    };
    Some(unit.format(value))
}

/// `buffer_min|max|avg(sensor, unit)`
fn eval_buffer_stat(args: &[Argument], ctx: &EvalCtx, stat: BufferStat) -> Option<String> {
    let sensor = arg_sensor_path(args, 0)?;
    let unit = arg_unit(args, 1)?;
    let value = match stat {
        BufferStat::Min => ctx.history.min(sensor),
        BufferStat::Max => ctx.history.max(sensor),
        BufferStat::Avg => ctx.history.avg(sensor),
    }
    .unwrap_or(0.0);
    Some(unit.format(value))
}

/// `nice_min(sensor)` / `nice_max(sensor)` — returns raw number as string
fn eval_nice_bound(args: &[Argument], ctx: &EvalCtx, bound: NiceBound) -> Option<String> {
    let sensor = arg_sensor_path(args, 0)?;
    let min_raw = ctx.history.min(sensor).unwrap_or(0.0);
    let max_raw = ctx.history.max(sensor).unwrap_or(0.0);
    if min_raw == 0.0 && max_raw == 0.0 {
        return Some("0".to_string());
    }
    let (nmin, nmax) = Unit::None.nice_bounds(min_raw, max_raw);
    Some(match bound {
        NiceBound::Min => format!("{}", nmin),
        NiceBound::Max => format!("{}", nmax),
    })
}

/// `nice_tick(sensor, unit, index, count)` — formatted label for nth tick
fn eval_nice_tick(args: &[Argument], ctx: &EvalCtx) -> Option<String> {
    let sensor = arg_sensor_path(args, 0)?;
    let unit = arg_unit(args, 1)?;
    let index = arg_number(args, 2)? as usize;
    let count = arg_number(args, 3)? as usize;
    let min = ctx.history.min(sensor).unwrap_or(0.0);
    let max = ctx.history.max(sensor).unwrap_or(0.0);
    if count == 0 {
        return Some(unit.format(min));
    }
    let ticks = unit.nice_ticks(min, max, count);
    if ticks.is_empty() {
        return Some(unit.format(0.0));
    }
    let clamped = index.min(ticks.len() - 1);
    Some(unit.format(ticks[clamped]))
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

    #[test]
    fn eval_format_value_sensor_and_unit() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 85.0;
        let history = SensorHistory::new();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{format_value(cpu.usage, %)}", &ctx);
        assert_eq!(result, "85 %");
    }

    #[test]
    fn eval_format_value_bytes() {
        let snapshot = SensorSnapshot::default();
        let history = SensorHistory::new();
        let mut hv = HashMap::new();
        hv.insert("network.bytes_per_sec".to_string(), 1_500_000.0);
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{format_value(network.bytes_per_sec, bytes/s)}", &ctx);
        assert_eq!(result, "1.4 MB/s");
    }

    #[test]
    fn eval_buffer_max_with_unit() {
        let snapshot = SensorSnapshot::default();
        let mut history = SensorHistory::new();
        history.register("cpu.usage");
        history.push_sample("cpu.usage", 10.0);
        history.push_sample("cpu.usage", 80.0);
        history.push_sample("cpu.usage", 40.0);
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{buffer_max(cpu.usage, %)}", &ctx);
        assert_eq!(result, "80 %");
    }

    #[test]
    fn eval_buffer_min_and_avg() {
        let snapshot = SensorSnapshot::default();
        let mut history = SensorHistory::new();
        history.register("cpu.usage");
        for v in [10.0, 20.0, 30.0, 40.0] {
            history.push_sample("cpu.usage", v);
        }
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let min_result = interpolate("{buffer_min(cpu.usage, %)}", &ctx);
        assert_eq!(min_result, "10 %");
        let avg_result = interpolate("{buffer_avg(cpu.usage, %)}", &ctx);
        assert_eq!(avg_result, "25 %");
    }

    #[test]
    fn eval_nice_min_max_returns_raw_numbers() {
        let snapshot = SensorSnapshot::default();
        let mut history = SensorHistory::new();
        history.register("cpu.usage");
        for v in [15.0, 85.0, 50.0] {
            history.push_sample("cpu.usage", v);
        }
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let max_result = interpolate("{nice_max(cpu.usage)}", &ctx);
        assert!(
            max_result.parse::<f64>().is_ok(),
            "nice_max should return a raw number, got {}",
            max_result
        );
        let min_result = interpolate("{nice_min(cpu.usage)}", &ctx);
        assert!(
            min_result.parse::<f64>().is_ok(),
            "nice_min should return a raw number, got {}",
            min_result
        );
    }

    #[test]
    fn eval_nice_tick_returns_formatted_label() {
        let snapshot = SensorSnapshot::default();
        let mut history = SensorHistory::new();
        history.register("cpu.usage");
        history.push_sample("cpu.usage", 0.0);
        history.push_sample("cpu.usage", 100.0);
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{nice_tick(cpu.usage, %, 2, 4)}", &ctx);
        assert!(
            result.contains('%'),
            "nice_tick should format with unit, got {}",
            result
        );
    }

    #[test]
    fn eval_empty_buffer_returns_zero_fallback() {
        let snapshot = SensorSnapshot::default();
        let mut history = SensorHistory::new();
        history.register("cpu.usage");
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{buffer_max(cpu.usage, %)}", &ctx);
        assert_eq!(result, "0 %");
    }
}
