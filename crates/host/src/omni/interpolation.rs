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
        "chart_polyline" => eval_chart_polyline(args, ctx),
        "chart_path" => eval_chart_path(args, ctx),
        "bar_height" => eval_bar_height(args, ctx),
        "bar_y" => eval_bar_y(args, ctx),
        "circumference" => eval_circumference(args),
        "ratio_dashoffset" => eval_ratio_dashoffset(args, ctx),
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

fn arg_sensor_path(args: &[Argument], idx: usize) -> Option<&str> {
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
    // Evenly interpolate `count` labels across the nice-rounded Y-axis range.
    // nice_ticks sometimes returns fewer values than requested (for tight
    // ranges), so using its length directly causes duplicate labels when
    // `index` exceeds `len-1`. Linear interpolation across the nice bounds
    // gives `count` distinct labels regardless of tick granularity.
    let (nice_min, nice_max) = unit.nice_bounds(min, max);
    let fraction = if count <= 1 {
        0.0
    } else {
        index.min(count - 1) as f64 / (count - 1) as f64
    };
    let value = nice_min + fraction * (nice_max - nice_min);
    Some(unit.format(value))
}

/// Compute scale bounds for a chart: use explicit min/max if provided,
/// otherwise derive from the buffer's min/max.
fn chart_bounds(args: &[Argument], ctx: &EvalCtx, sensor: &str) -> Option<(f64, f64)> {
    if args.len() >= 5 {
        let min = arg_number(args, 3)?;
        let max = arg_number(args, 4)?;
        return Some((min, max));
    }
    let min = ctx.history.min(sensor).unwrap_or(0.0);
    let max = ctx.history.max(sensor).unwrap_or(0.0);
    if min == max {
        // Flat line — give it a 10% pad
        let pad = (min.abs() * 0.1).max(1.0);
        Some((min - pad, max + pad))
    } else {
        Some((min, max))
    }
}

/// `chart_polyline(sensor, width, height)` or `chart_polyline(sensor, w, h, min, max)`
fn eval_chart_polyline(args: &[Argument], ctx: &EvalCtx) -> Option<String> {
    let sensor = arg_sensor_path(args, 0)?;
    let width = arg_number(args, 1)?;
    let height = arg_number(args, 2)?;
    let buffer = ctx.history.buffer(sensor)?;
    if buffer.is_empty() {
        return Some(String::new());
    }
    let (min, max) = chart_bounds(args, ctx, sensor)?;
    let range = (max - min).abs();
    if range == 0.0 {
        return Some(String::new());
    }
    let n = buffer.len();
    let step_x = if n > 1 { width / (n - 1) as f64 } else { 0.0 };
    let mut parts = Vec::with_capacity(n);
    for (i, v) in buffer.iter().enumerate() {
        let x = i as f64 * step_x;
        let y = height - ((v - min) / range) * height;
        parts.push(format!("{:.1},{:.1}", x, y));
    }
    Some(parts.join(" "))
}

/// `chart_path(sensor, width, height)` — SVG path `d` string for `<path>`
fn eval_chart_path(args: &[Argument], ctx: &EvalCtx) -> Option<String> {
    let sensor = arg_sensor_path(args, 0)?;
    let width = arg_number(args, 1)?;
    let height = arg_number(args, 2)?;
    let buffer = ctx.history.buffer(sensor)?;
    if buffer.is_empty() {
        return Some(String::new());
    }
    let (min, max) = chart_bounds(args, ctx, sensor)?;
    let range = (max - min).abs();
    if range == 0.0 {
        return Some(String::new());
    }
    let n = buffer.len();
    let step_x = if n > 1 { width / (n - 1) as f64 } else { 0.0 };
    let points: Vec<(f64, f64)> = buffer
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = i as f64 * step_x;
            let y = height - ((v - min) / range) * height;
            (x, y)
        })
        .collect();
    if points.is_empty() {
        return Some(String::new());
    }
    let mut d = format!("M{:.1},{:.1}", points[0].0, points[0].1);
    for pt in points.iter().skip(1) {
        d.push_str(&format!(" L{:.1},{:.1}", pt.0, pt.1));
    }
    Some(d)
}

/// `bar_height(sensor, height, min, max)`
fn eval_bar_height(args: &[Argument], ctx: &EvalCtx) -> Option<String> {
    let sensor = arg_sensor_path(args, 0)?;
    let h = arg_number(args, 1)?;
    let min = arg_number(args, 2)?;
    let max = arg_number(args, 3)?;
    let value = sensor_map::get_sensor_value_f64(sensor, ctx.snapshot, ctx.hwinfo_values)
        .unwrap_or(0.0);
    let range = max - min;
    if range == 0.0 {
        return Some("0".to_string());
    }
    let clamped = value.max(min).min(max);
    let height = ((clamped - min) / range) * h;
    Some(format!("{}", height as i64))
}

/// `bar_y(sensor, height, min, max)` — y-coordinate for top of bar
fn eval_bar_y(args: &[Argument], ctx: &EvalCtx) -> Option<String> {
    let sensor = arg_sensor_path(args, 0)?;
    let h = arg_number(args, 1)?;
    let min = arg_number(args, 2)?;
    let max = arg_number(args, 3)?;
    let value = sensor_map::get_sensor_value_f64(sensor, ctx.snapshot, ctx.hwinfo_values)
        .unwrap_or(0.0);
    let range = max - min;
    if range == 0.0 {
        return Some(format!("{}", h as i64));
    }
    let clamped = value.max(min).min(max);
    let bar_h = ((clamped - min) / range) * h;
    Some(format!("{}", (h - bar_h) as i64))
}

/// `circumference(r)` → 2πr
fn eval_circumference(args: &[Argument]) -> Option<String> {
    let r = arg_number(args, 0)?;
    let c = 2.0 * std::f64::consts::PI * r;
    Some(format!("{:.4}", c))
}

/// `ratio_dashoffset(value_sensor, total_sensor, r)`
fn eval_ratio_dashoffset(args: &[Argument], ctx: &EvalCtx) -> Option<String> {
    let value_sensor = arg_sensor_path(args, 0)?;
    let total_sensor = arg_sensor_path(args, 1)?;
    let r = arg_number(args, 2)?;
    let value = sensor_map::get_sensor_value_f64(value_sensor, ctx.snapshot, ctx.hwinfo_values)
        .unwrap_or(0.0);
    let total = sensor_map::get_sensor_value_f64(total_sensor, ctx.snapshot, ctx.hwinfo_values)
        .unwrap_or(0.0);
    let ratio = if total > 0.0 {
        (value / total).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let circumference = 2.0 * std::f64::consts::PI * r;
    let offset = circumference * (1.0 - ratio);
    Some(format!("{:.4}", offset))
}

/// Scan `input` for `{...}` expressions and replace each with its
/// evaluated result.
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

    #[test]
    fn eval_chart_polyline_returns_points_string() {
        let snapshot = SensorSnapshot::default();
        let mut history = SensorHistory::new();
        history.register("cpu.usage");
        history.push_sample("cpu.usage", 0.0);
        history.push_sample("cpu.usage", 50.0);
        history.push_sample("cpu.usage", 100.0);
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{chart_polyline(cpu.usage, 200, 60)}", &ctx);
        let parts: Vec<&str> = result.split(' ').collect();
        assert_eq!(parts.len(), 3, "expected 3 points, got {:?}", parts);
        for part in &parts {
            let coords: Vec<&str> = part.split(',').collect();
            assert_eq!(coords.len(), 2);
            coords[0].parse::<f64>().expect("x should be a number");
            coords[1].parse::<f64>().expect("y should be a number");
        }
    }

    #[test]
    fn eval_chart_polyline_empty_buffer_returns_empty_string() {
        let snapshot = SensorSnapshot::default();
        let mut history = SensorHistory::new();
        history.register("cpu.usage");
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{chart_polyline(cpu.usage, 200, 60)}", &ctx);
        assert_eq!(result, "");
    }

    #[test]
    fn eval_chart_polyline_fixed_scale() {
        let snapshot = SensorSnapshot::default();
        let mut history = SensorHistory::new();
        history.register("cpu.usage");
        history.push_sample("cpu.usage", 50.0);
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        // With min=0, max=100, height=60, value 50 should map to y=30 (midpoint).
        // Single sample → x=0, y=30 (step_x is 0 when n=1).
        let result = interpolate("{chart_polyline(cpu.usage, 200, 60, 0, 100)}", &ctx);
        assert!(
            result.contains(",30") || result.contains(",30.0"),
            "expected y=30 in points: {}",
            result
        );
    }

    #[test]
    fn eval_bar_height_and_y() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.cpu.total_usage_percent = 50.0;
        let history = SensorHistory::new();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        // cpu.usage = 50, min=0, max=100, h=60 → bar height = 30
        let height_result = interpolate("{bar_height(cpu.usage, 60, 0, 100)}", &ctx);
        assert_eq!(height_result, "30");
        // bar_y = h - bar_height = 60 - 30 = 30
        let y_result = interpolate("{bar_y(cpu.usage, 60, 0, 100)}", &ctx);
        assert_eq!(y_result, "30");
    }

    #[test]
    fn eval_circumference() {
        let snapshot = SensorSnapshot::default();
        let history = SensorHistory::new();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        let result = interpolate("{circumference(40)}", &ctx);
        let v: f64 = result.parse().unwrap();
        let expected = 2.0 * std::f64::consts::PI * 40.0;
        assert!(
            (v - expected).abs() < 0.01,
            "got {} expected {}",
            v,
            expected
        );
    }

    #[test]
    fn eval_ratio_dashoffset() {
        let mut snapshot = SensorSnapshot::default();
        snapshot.ram.used_mb = 8192;
        snapshot.ram.total_mb = 16384;
        let history = SensorHistory::new();
        let hv = HashMap::new();
        let hu = HashMap::new();
        let ctx = make_ctx(&snapshot, &history, &hv, &hu);
        // ratio = 0.5 → dashoffset = circumference * (1 - 0.5) = circumference * 0.5
        let result = interpolate("{ratio_dashoffset(ram.used, ram.total, 40)}", &ctx);
        let v: f64 = result.parse().unwrap();
        let circ = 2.0 * std::f64::consts::PI * 40.0;
        assert!(
            (v - circ * 0.5).abs() < 0.01,
            "got {} expected {}",
            v,
            circ * 0.5
        );
    }
}
