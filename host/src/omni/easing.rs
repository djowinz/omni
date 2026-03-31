//! CSS easing functions for transition interpolation.

/// An easing function that maps progress (0.0–1.0) to an output value (0.0–1.0).
#[derive(Debug, Clone)]
pub enum EasingFunction {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    CubicBezier(f64, f64, f64, f64), // (x1, y1, x2, y2)
}

impl EasingFunction {
    /// Parse a CSS easing function name or cubic-bezier(...).
    pub fn parse(s: &str) -> Self {
        match s.trim() {
            "linear" => Self::Linear,
            "ease" => Self::Ease,
            "ease-in" => Self::EaseIn,
            "ease-out" => Self::EaseOut,
            "ease-in-out" => Self::EaseInOut,
            s if s.starts_with("cubic-bezier(") => {
                if let Some(inner) = s.strip_prefix("cubic-bezier(").and_then(|s| s.strip_suffix(')')) {
                    let parts: Vec<f64> = inner.split(',')
                        .filter_map(|p| p.trim().parse().ok())
                        .collect();
                    if parts.len() == 4 {
                        return Self::CubicBezier(parts[0], parts[1], parts[2], parts[3]);
                    }
                }
                Self::Ease // fallback for malformed cubic-bezier
            }
            _ => Self::Ease, // default
        }
    }

    /// Apply the easing function to a progress value (0.0–1.0).
    /// Returns the eased output (0.0–1.0).
    pub fn apply(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Ease => cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
            Self::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
            Self::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
            Self::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
            Self::CubicBezier(x1, y1, x2, y2) => cubic_bezier(*x1, *y1, *x2, *y2, t),
        }
    }
}

/// Evaluate a cubic Bezier curve at the given x-axis progress.
/// The curve has control points (0,0), (x1,y1), (x2,y2), (1,1).
/// Uses Newton-Raphson to find parameter u where B_x(u) = t, then returns B_y(u).
fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }

    // Newton-Raphson: find u such that bezier_component(x1, x2, u) == t
    let mut guess = t;
    for _ in 0..8 {
        let x = bezier_component(x1, x2, guess) - t;
        if x.abs() < 1e-6 {
            break;
        }
        let dx = bezier_derivative(x1, x2, guess);
        if dx.abs() < 1e-6 {
            break;
        }
        guess -= x / dx;
    }

    bezier_component(y1, y2, guess)
}

/// Evaluate the parametric cubic Bezier for one component:
/// B(u) = 3(1-u)^2 * u * p1 + 3(1-u) * u^2 * p2 + u^3
fn bezier_component(p1: f64, p2: f64, u: f64) -> f64 {
    let one_minus_u = 1.0 - u;
    3.0 * one_minus_u * one_minus_u * u * p1 + 3.0 * one_minus_u * u * u * p2 + u * u * u
}

/// Derivative of bezier_component with respect to u:
/// B'(u) = 3(1-u)^2 * p1 + 6(1-u)*u*(p2 - p1) + 3*u^2*(1 - p2)
fn bezier_derivative(p1: f64, p2: f64, u: f64) -> f64 {
    let one_minus_u = 1.0 - u;
    3.0 * one_minus_u * one_minus_u * p1 + 6.0 * one_minus_u * u * (p2 - p1) + 3.0 * u * u * (1.0 - p2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_at_zero() {
        assert!((EasingFunction::Linear.apply(0.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn linear_at_half() {
        assert!((EasingFunction::Linear.apply(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn linear_at_one() {
        assert!((EasingFunction::Linear.apply(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ease_endpoints() {
        assert!((EasingFunction::Ease.apply(0.0) - 0.0).abs() < 1e-6);
        assert!((EasingFunction::Ease.apply(1.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn ease_midpoint_frontloaded() {
        // CSS "ease" front-loads progress: at t=0.5, output should be ~0.8
        let val = EasingFunction::Ease.apply(0.5);
        assert!(
            (val - 0.8).abs() < 0.1,
            "ease at 0.5 should be approximately 0.8, got {}",
            val
        );
    }

    #[test]
    fn ease_in_slow_start() {
        // EaseIn at t=0.5 should be less than 0.5 (slow start)
        let val = EasingFunction::EaseIn.apply(0.5);
        assert!(
            val < 0.5,
            "ease-in at 0.5 should be < 0.5, got {}",
            val
        );
    }

    #[test]
    fn ease_out_fast_start() {
        // EaseOut at t=0.5 should be greater than 0.5 (fast start)
        let val = EasingFunction::EaseOut.apply(0.5);
        assert!(
            val > 0.5,
            "ease-out at 0.5 should be > 0.5, got {}",
            val
        );
    }

    #[test]
    fn ease_in_out_symmetric() {
        // EaseInOut at t=0.5 should be approximately 0.5 (symmetric)
        let val = EasingFunction::EaseInOut.apply(0.5);
        assert!(
            (val - 0.5).abs() < 0.05,
            "ease-in-out at 0.5 should be approximately 0.5, got {}",
            val
        );
    }

    #[test]
    fn parse_ease() {
        match EasingFunction::parse("ease") {
            EasingFunction::Ease => {}
            other => panic!("expected Ease, got {:?}", other),
        }
    }

    #[test]
    fn parse_cubic_bezier() {
        match EasingFunction::parse("cubic-bezier(0.1, 0.2, 0.3, 0.4)") {
            EasingFunction::CubicBezier(x1, y1, x2, y2) => {
                assert!((x1 - 0.1).abs() < 1e-6);
                assert!((y1 - 0.2).abs() < 1e-6);
                assert!((x2 - 0.3).abs() < 1e-6);
                assert!((y2 - 0.4).abs() < 1e-6);
            }
            other => panic!("expected CubicBezier, got {:?}", other),
        }
    }

    #[test]
    fn parse_unknown_defaults_to_ease() {
        match EasingFunction::parse("unknown") {
            EasingFunction::Ease => {}
            other => panic!("expected Ease (default), got {:?}", other),
        }
    }

    #[test]
    fn parse_all_named_variants() {
        assert!(matches!(EasingFunction::parse("linear"), EasingFunction::Linear));
        assert!(matches!(EasingFunction::parse("ease-in"), EasingFunction::EaseIn));
        assert!(matches!(EasingFunction::parse("ease-out"), EasingFunction::EaseOut));
        assert!(matches!(EasingFunction::parse("ease-in-out"), EasingFunction::EaseInOut));
    }

    #[test]
    fn cubic_bezier_custom_values() {
        // A custom cubic-bezier should produce a value between 0 and 1
        let easing = EasingFunction::CubicBezier(0.1, 0.7, 0.9, 0.3);
        let val = easing.apply(0.5);
        assert!(val >= 0.0 && val <= 1.0, "custom bezier at 0.5 = {}", val);
        assert!((easing.apply(0.0) - 0.0).abs() < 1e-6);
        assert!((easing.apply(1.0) - 1.0).abs() < 1e-6);
    }
}
