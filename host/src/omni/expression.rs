//! Tokenizer + recursive descent parser/evaluator for condition expressions
//! against sensor data.
//!
//! Grammar:
//! ```text
//! expr     → or_expr
//! or_expr  → and_expr ( "||" and_expr )*
//! and_expr → not_expr ( "&&" not_expr )*
//! not_expr → "!" not_expr | compare
//! compare  → add_expr ( (">" | "<" | ">=" | "<=" | "==" | "!=") add_expr )?
//! add_expr → mul_expr ( ("+" | "-") mul_expr )*
//! mul_expr → primary ( ("*" | "/") primary )*
//! primary  → NUMBER | SENSOR_PATH | "(" expr ")"
//! ```

use omni_shared::SensorSnapshot;
use std::sync::Mutex;
use std::collections::HashSet;

static WARNED_EXPRS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn warn_once(expr: &str, reason: &str) {
    let mut guard = WARNED_EXPRS.lock().unwrap_or_else(|e| e.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    if set.insert(expr.to_string()) {
        tracing::warn!("expression eval failed for {:?}: {}", expr, reason);
    }
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f64),
    SensorPath(String),
    Plus,
    Minus,
    Star,
    Slash,
    Gt,
    Lt,
    Gte,
    Lte,
    Eq,
    Neq,
    And,
    Or,
    Not,
    LParen,
    RParen,
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        // Skip whitespace
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Two-char operators
        if i + 1 < len {
            let two = (c, chars[i + 1]);
            match two {
                ('|', '|') => { tokens.push(Token::Or); i += 2; continue; }
                ('&', '&') => { tokens.push(Token::And); i += 2; continue; }
                ('>', '=') => { tokens.push(Token::Gte); i += 2; continue; }
                ('<', '=') => { tokens.push(Token::Lte); i += 2; continue; }
                ('=', '=') => { tokens.push(Token::Eq); i += 2; continue; }
                ('!', '=') => { tokens.push(Token::Neq); i += 2; continue; }
                _ => {}
            }
        }

        // Single-char operators
        match c {
            '+' => { tokens.push(Token::Plus); i += 1; continue; }
            '-' => { tokens.push(Token::Minus); i += 1; continue; }
            '*' => { tokens.push(Token::Star); i += 1; continue; }
            '/' => { tokens.push(Token::Slash); i += 1; continue; }
            '>' => { tokens.push(Token::Gt); i += 1; continue; }
            '<' => { tokens.push(Token::Lt); i += 1; continue; }
            '!' => { tokens.push(Token::Not); i += 1; continue; }
            '(' => { tokens.push(Token::LParen); i += 1; continue; }
            ')' => { tokens.push(Token::RParen); i += 1; continue; }
            _ => {}
        }

        // Number (digits, possibly with decimal point)
        if c.is_ascii_digit() || (c == '.' && i + 1 < len && chars[i + 1].is_ascii_digit()) {
            let start = i;
            while i < len && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            let num: f64 = num_str.parse().map_err(|_| format!("invalid number: {}", num_str))?;
            tokens.push(Token::Number(num));
            continue;
        }

        // Sensor path: alphanumeric, dots, hyphens, underscores
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < len && (chars[i].is_ascii_alphanumeric() || chars[i] == '.' || chars[i] == '-' || chars[i] == '_') {
                i += 1;
            }
            let path: String = chars[start..i].iter().collect();
            tokens.push(Token::SensorPath(path));
            continue;
        }

        return Err(format!("unexpected character: {:?}", c));
    }

    Ok(tokens)
}

// ---------------------------------------------------------------------------
// Parser / Evaluator
// ---------------------------------------------------------------------------

/// Result of evaluating an expression node. We track numeric values and convert
/// to bool when needed (nonzero = true).
#[derive(Debug, Clone, Copy)]
enum Value {
    Num(f64),
    Bool(bool),
}

impl Value {
    fn as_f64(self) -> f64 {
        match self {
            Value::Num(n) => n,
            Value::Bool(b) => if b { 1.0 } else { 0.0 },
        }
    }

    fn as_bool(self) -> bool {
        match self {
            Value::Bool(b) => b,
            Value::Num(n) => n != 0.0,
        }
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    snapshot: &'a SensorSnapshot,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token], snapshot: &'a SensorSnapshot) -> Self {
        Self { tokens, pos: 0, snapshot }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<Token> {
        let tok = self.tokens.get(self.pos).cloned();
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn expect_rparen(&mut self) -> Result<(), String> {
        match self.peek() {
            Some(Token::RParen) => { self.pos += 1; Ok(()) }
            _ => Err("expected ')'".to_string()),
        }
    }

    // expr → or_expr
    fn expr(&mut self) -> Result<Value, String> {
        self.or_expr()
    }

    // or_expr → and_expr ( "||" and_expr )*
    fn or_expr(&mut self) -> Result<Value, String> {
        let mut left = self.and_expr()?;
        while self.peek() == Some(&Token::Or) {
            self.advance();
            let right = self.and_expr()?;
            left = Value::Bool(left.as_bool() || right.as_bool());
        }
        Ok(left)
    }

    // and_expr → not_expr ( "&&" not_expr )*
    fn and_expr(&mut self) -> Result<Value, String> {
        let mut left = self.not_expr()?;
        while self.peek() == Some(&Token::And) {
            self.advance();
            let right = self.not_expr()?;
            left = Value::Bool(left.as_bool() && right.as_bool());
        }
        Ok(left)
    }

    // not_expr → "!" not_expr | compare
    fn not_expr(&mut self) -> Result<Value, String> {
        if self.peek() == Some(&Token::Not) {
            self.advance();
            let val = self.not_expr()?;
            return Ok(Value::Bool(!val.as_bool()));
        }
        self.compare()
    }

    // compare → add_expr ( (">" | "<" | ">=" | "<=" | "==" | "!=") add_expr )?
    fn compare(&mut self) -> Result<Value, String> {
        let left = self.add_expr()?;
        let op = match self.peek() {
            Some(Token::Gt) => Some(Token::Gt),
            Some(Token::Lt) => Some(Token::Lt),
            Some(Token::Gte) => Some(Token::Gte),
            Some(Token::Lte) => Some(Token::Lte),
            Some(Token::Eq) => Some(Token::Eq),
            Some(Token::Neq) => Some(Token::Neq),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let right = self.add_expr()?;
            let (l, r) = (left.as_f64(), right.as_f64());
            let result = match op {
                Token::Gt => l > r,
                Token::Lt => l < r,
                Token::Gte => l >= r,
                Token::Lte => l <= r,
                Token::Eq => (l - r).abs() < f64::EPSILON,
                Token::Neq => (l - r).abs() >= f64::EPSILON,
                _ => unreachable!(),
            };
            return Ok(Value::Bool(result));
        }
        Ok(left)
    }

    // add_expr → mul_expr ( ("+" | "-") mul_expr )*
    fn add_expr(&mut self) -> Result<Value, String> {
        let mut left = self.mul_expr()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.advance();
                    let right = self.mul_expr()?;
                    left = Value::Num(left.as_f64() + right.as_f64());
                }
                Some(Token::Minus) => {
                    self.advance();
                    let right = self.mul_expr()?;
                    left = Value::Num(left.as_f64() - right.as_f64());
                }
                _ => break,
            }
        }
        Ok(left)
    }

    // mul_expr → primary ( ("*" | "/") primary )*
    fn mul_expr(&mut self) -> Result<Value, String> {
        let mut left = self.primary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.advance();
                    let right = self.primary()?;
                    left = Value::Num(left.as_f64() * right.as_f64());
                }
                Some(Token::Slash) => {
                    self.advance();
                    let right = self.primary()?;
                    let r = right.as_f64();
                    if r == 0.0 {
                        return Err("division by zero".to_string());
                    }
                    left = Value::Num(left.as_f64() / r);
                }
                _ => break,
            }
        }
        Ok(left)
    }

    // primary → NUMBER | SENSOR_PATH | "(" expr ")"
    fn primary(&mut self) -> Result<Value, String> {
        match self.advance() {
            Some(Token::Number(n)) => Ok(Value::Num(n)),
            Some(Token::SensorPath(path)) => {
                let s = super::sensor_map::get_sensor_value(&path, self.snapshot);
                let val = s.parse::<f64>().unwrap_or(0.0);
                Ok(Value::Num(val))
            }
            Some(Token::LParen) => {
                let val = self.expr()?;
                self.expect_rparen()?;
                Ok(val)
            }
            Some(tok) => Err(format!("unexpected token: {:?}", tok)),
            None => Err("unexpected end of expression".to_string()),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Evaluate a condition expression against sensor data. Returns true/false.
/// Malformed expressions return false and log a warning (once per unique expr).
pub fn eval_condition(expr: &str, snapshot: &SensorSnapshot) -> bool {
    match eval_inner(expr, snapshot) {
        Ok(val) => val.as_bool(),
        Err(reason) => {
            warn_once(expr, &reason);
            false
        }
    }
}

/// Evaluate an expression to a numeric value.
/// Used for interpolation targets (e.g., computing percentages).
pub fn eval_numeric(expr: &str, snapshot: &SensorSnapshot) -> Option<f64> {
    match eval_inner(expr, snapshot) {
        Ok(val) => Some(val.as_f64()),
        Err(reason) => {
            warn_once(expr, &reason);
            None
        }
    }
}

fn eval_inner(expr: &str, snapshot: &SensorSnapshot) -> Result<Value, String> {
    let tokens = tokenize(expr)?;
    if tokens.is_empty() {
        return Err("empty expression".to_string());
    }
    let mut parser = Parser::new(&tokens, snapshot);
    let result = parser.expr()?;
    if parser.pos < parser.tokens.len() {
        return Err(format!("unexpected trailing tokens at position {}", parser.pos));
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use omni_shared::SensorSnapshot;

    fn make_snapshot(gpu_temp: f32, gpu_usage: f32, cpu_usage: f32, vram_used: u32, vram_total: u32) -> SensorSnapshot {
        let mut s = SensorSnapshot::default();
        s.gpu.temp_c = gpu_temp;
        s.gpu.usage_percent = gpu_usage;
        s.cpu.total_usage_percent = cpu_usage;
        s.gpu.vram_used_mb = vram_used;
        s.gpu.vram_total_mb = vram_total;
        s
    }

    #[test]
    fn simple_gt_true() {
        // gpu.temp formatted as "85" (f32 85.0 → "{:.0}" → "85")
        let snap = make_snapshot(85.0, 0.0, 0.0, 0, 0);
        assert!(eval_condition("gpu.temp > 80", &snap));
    }

    #[test]
    fn simple_gt_false() {
        let snap = make_snapshot(70.0, 0.0, 0.0, 0, 0);
        assert!(!eval_condition("gpu.temp > 80", &snap));
    }

    #[test]
    fn vram_ratio() {
        // gpu.vram.used = "9000", gpu.vram.total = "10000"
        // 9000 / 10000 = 0.9 > 0.9 → false (not strictly greater)
        let snap = make_snapshot(0.0, 0.0, 0.0, 9001, 10000);
        assert!(eval_condition("gpu.vram.used / gpu.vram.total > 0.9", &snap));

        let snap2 = make_snapshot(0.0, 0.0, 0.0, 8000, 10000);
        assert!(!eval_condition("gpu.vram.used / gpu.vram.total > 0.9", &snap2));
    }

    #[test]
    fn logical_and() {
        // cpu.usage formatted as "{:.0}" so 95.0 → "95", gpu.usage → "95"
        let snap_both = make_snapshot(0.0, 95.0, 95.0, 0, 0);
        assert!(eval_condition("cpu.usage > 90 && gpu.usage > 90", &snap_both));

        let snap_one = make_snapshot(0.0, 50.0, 95.0, 0, 0);
        assert!(!eval_condition("cpu.usage > 90 && gpu.usage > 90", &snap_one));
    }

    #[test]
    fn logical_or() {
        // fps returns "N/A" → 0.0, gpu.temp returns temp
        let snap = make_snapshot(96.0, 0.0, 0.0, 0, 0);
        // fps=0 < 30 → true || gpu.temp > 95 → true, so true
        assert!(eval_condition("fps < 30 || gpu.temp > 95", &snap));

        let snap2 = make_snapshot(50.0, 0.0, 0.0, 0, 0);
        // fps=0 < 30 → true, so still true
        assert!(eval_condition("fps < 30 || gpu.temp > 95", &snap2));

        // Neither true: need fps >= 30 and gpu.temp <= 95
        // fps is always "N/A" → 0.0, so fps < 30 is always true in default snapshot.
        // Use a numeric expression instead to test "neither":
        // gpu.usage < 30 || gpu.temp > 95 — both false
        let snap3 = make_snapshot(50.0, 50.0, 0.0, 0, 0);
        assert!(!eval_condition("gpu.usage < 30 || gpu.temp > 95", &snap3));
    }

    #[test]
    fn logical_not() {
        // fps = "N/A" → 0.0, !(0 > 60) → !false → true
        let snap = make_snapshot(0.0, 0.0, 0.0, 0, 0);
        assert!(eval_condition("!(fps > 60)", &snap));
    }

    #[test]
    fn parenthesized_avg() {
        // cpu.usage = "85", gpu.usage = "95" → (85+95)/2 = 90 > 80 → true
        let snap = make_snapshot(0.0, 95.0, 85.0, 0, 0);
        assert!(eval_condition("(cpu.usage + gpu.usage) / 2 > 80", &snap));
    }

    #[test]
    fn equality() {
        // gpu.temp = 60.0 → "60", 60 == 60 → true
        let snap = make_snapshot(60.0, 0.0, 0.0, 0, 0);
        assert!(eval_condition("gpu.temp == 60", &snap));
        assert!(!eval_condition("gpu.temp == 61", &snap));
    }

    #[test]
    fn nested_parens() {
        // ((gpu.temp > 80) && (cpu.usage > 90)) || fps < 30
        // gpu.temp=85 > 80 → true, cpu.usage=95 > 90 → true, so left side true
        let snap = make_snapshot(85.0, 0.0, 95.0, 0, 0);
        assert!(eval_condition("((gpu.temp > 80) && (cpu.usage > 90)) || fps < 30", &snap));

        // gpu.temp=70 > 80 → false, cpu.usage=50 > 90 → false, fps=N/A=0 < 30 → true
        let snap2 = make_snapshot(70.0, 0.0, 50.0, 0, 0);
        assert!(eval_condition("((gpu.temp > 80) && (cpu.usage > 90)) || fps < 30", &snap2));
    }

    #[test]
    fn invalid_expression_no_panic() {
        let snap = SensorSnapshot::default();
        assert!(!eval_condition("invalid !!@ garbage", &snap));
        assert!(!eval_condition("", &snap));
        assert!(!eval_condition("(((", &snap));
    }

    #[test]
    fn division_by_zero_no_panic() {
        let snap = SensorSnapshot::default();
        assert!(!eval_condition("100 / 0 > 1", &snap));
        assert_eq!(eval_numeric("100 / 0", &snap), None);
    }

    #[test]
    fn eval_numeric_basic() {
        let snap = make_snapshot(75.0, 0.0, 0.0, 0, 0);
        let val = eval_numeric("gpu.temp + 5", &snap).unwrap();
        assert!((val - 80.0).abs() < f64::EPSILON);
    }

    #[test]
    fn eval_numeric_invalid() {
        let snap = SensorSnapshot::default();
        assert_eq!(eval_numeric("", &snap), None);
    }
}
