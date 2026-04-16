//! Real `ThemeSwap` implementation that drives the Ultralight renderer
//! via the privileged bootstrap's `__omni_set_theme` function.
//!
//! Sub-spec #002 shipped the JS side (see `crates/host/src/omni/bootstrap.js`
//! L156–163 — `window.__omni_set_theme(vars)` walks a JS object and calls
//! `root.style.setProperty("--" + k, v)`). This module wires the Rust side
//! for sub-spec #021's `explorer.preview` WebSocket handler, replacing the
//! `NoopThemeSwap` that `build_share_context` installed while the renderer
//! surface was still unfinished.
//!
//! **Pending-theme-slot pattern.** Ultralight's `evaluate_script` MUST only
//! be called from the main render thread; `ThemeSwap::apply` and
//! `ThemeSwap::revert`, however, can fire from any Tokio worker (auto-revert
//! task, WS dispatch). The impl therefore never evaluates JS directly — it
//! parses the candidate CSS for `:root{--var:value;…}` custom properties and
//! stashes the resulting `HashMap<String, String>` in an `Arc<Mutex<…>>` slot
//! shared with the main render loop. The loop calls [`drain_pending_js`]
//! once per frame and, if non-empty, emits the
//! `__omni_set_theme({…})` invocation through `UlRenderer::evaluate_script`.
//!
//! **Snapshot payload = CSS bytes.** `ThemeSwap::snapshot` returns the
//! baseline CSS bytes as documented by the trait (`Vec<u8>`). Revert feeds
//! those same bytes back through the apply path so restoration reuses one
//! extraction pipeline.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::share::preview::ThemeSwap;

/// Pending custom-property override produced by [`ThemeSwapImpl::apply`] or
/// [`ThemeSwapImpl::revert`], drained by the main render loop each frame via
/// [`drain_pending_js`]. The map keys are custom-property names WITHOUT the
/// leading `--` (the bootstrap's `__omni_set_theme` re-prepends `"--"`).
#[derive(Debug, Clone)]
pub struct PendingTheme {
    pub vars: HashMap<String, String>,
}

/// Shared single-slot mailbox between `ThemeSwapImpl` and the main render
/// loop. Only the most-recent pending theme survives — older in-flight
/// overrides are overwritten.
pub type PendingSlot = Arc<Mutex<Option<PendingTheme>>>;

/// Real `ThemeSwap` — parses CSS, stashes the resulting custom-property map
/// in a shared slot. The main render loop drains the slot and converts it to
/// a `__omni_set_theme` script invocation.
pub struct ThemeSwapImpl {
    /// Current overlay's theme CSS bytes (captured at host startup).
    /// Returned unmodified by [`ThemeSwap::snapshot`]; revert feeds these
    /// back through the apply path to restore baseline.
    baseline_css: Vec<u8>,
    pending: PendingSlot,
}

impl ThemeSwapImpl {
    /// Construct a `ThemeSwapImpl` with the given baseline CSS and shared
    /// pending slot. The slot is also the main-loop drain target — callers
    /// typically construct it as `Arc::new(Mutex::new(None))` and clone into
    /// both this impl and the render loop.
    pub fn new(baseline_css: Vec<u8>, pending: PendingSlot) -> Self {
        Self {
            baseline_css,
            pending,
        }
    }
}

impl ThemeSwap for ThemeSwapImpl {
    fn snapshot(&self) -> Vec<u8> {
        self.baseline_css.clone()
    }

    fn apply(&self, css: &[u8]) -> Result<(), String> {
        let vars = extract_custom_properties(css)?;
        let mut slot = self
            .pending
            .lock()
            .map_err(|_| "pending mutex poisoned".to_string())?;
        *slot = Some(PendingTheme { vars });
        Ok(())
    }

    fn revert(&self, snapshot: &[u8]) -> Result<(), String> {
        // Revert IS "apply the baseline" — the snapshot is the baseline CSS
        // bytes we returned from `snapshot()`. Routing through `apply` keeps
        // one extraction pipeline.
        self.apply(snapshot)
    }
}

/// Drain the pending slot and format the payload as a
/// `__omni_set_theme({…})` JS invocation guarded on function existence.
/// Returns `None` if the slot is empty (common case — most frames have no
/// pending theme change).
///
/// Called by the main render loop once per frame, just before / after the
/// existing `__omni_update(values)` invocation, and handed to
/// `UlRenderer::evaluate_script`.
///
/// Poisoned-mutex fallback: returns `None` (best-effort; a poisoned slot
/// means some other thread panicked while holding the lock, in which case
/// the wider host is probably already torn down).
pub fn drain_pending_js(slot: &PendingSlot) -> Option<String> {
    let mut guard = slot.lock().ok()?;
    let pending = guard.take()?;
    let json = serde_json::to_string(&pending.vars).ok()?;
    Some(format!(
        "if(window.__omni_set_theme){{window.__omni_set_theme({json});}}"
    ))
}

/// Parse CSS bytes and extract `:root { --foo: bar; … }` custom-property
/// declarations.
///
/// Returns `HashMap<name_without_dashes, value_string>`. Non-custom-property
/// declarations and non-`:root` selectors are silently ignored per the
/// module doc contract — theme files legitimately carry ordinary class
/// rules that are rendered via the normal stylesheet load path, not the
/// custom-property injection seam.
///
/// Errors:
/// - `invalid UTF-8: …` when `css` is not valid UTF-8.
/// - `CSS parse failed: …` when lightningcss rejects the stylesheet.
/// - `serialize value: …` when converting a property value to string fails.
fn extract_custom_properties(css: &[u8]) -> Result<HashMap<String, String>, String> {
    use lightningcss::printer::PrinterOptions;
    use lightningcss::properties::custom::CustomPropertyName;
    use lightningcss::properties::Property;
    use lightningcss::rules::CssRule;
    use lightningcss::selector::Component;
    use lightningcss::stylesheet::{ParserOptions, StyleSheet};

    let css_str = std::str::from_utf8(css).map_err(|e| format!("invalid UTF-8: {e}"))?;
    let sheet = StyleSheet::parse(css_str, ParserOptions::default())
        .map_err(|e| format!("CSS parse failed: {e}"))?;

    let mut out = HashMap::new();
    for rule in &sheet.rules.0 {
        let CssRule::Style(style_rule) = rule else {
            continue;
        };

        // A theme file's baseline selector is a bare `:root`. We accept any
        // selector whose raw-match-order components reduce to exactly
        // `Component::Root` — `:root:hover`, `html:root`, and compound forms
        // are deliberately skipped because their applied-conditions semantics
        // don't map cleanly onto the always-on bootstrap set-property path.
        let is_root = style_rule.selectors.0.iter().any(|selector| {
            let mut components = selector.iter_raw_match_order();
            matches!(
                (components.next(), components.next()),
                (Some(Component::Root), None)
            )
        });
        if !is_root {
            continue;
        }

        // Emit both `declarations` and `important_declarations` — user themes
        // occasionally mark overrides `!important` and still expect them to
        // apply. Bootstrap's `setProperty(name, value)` ignores priority,
        // which is acceptable here (preview is transient by design).
        for decl in style_rule
            .declarations
            .declarations
            .iter()
            .chain(style_rule.declarations.important_declarations.iter())
        {
            let Property::Custom(custom) = decl else {
                continue;
            };
            let CustomPropertyName::Custom(dashed) = &custom.name else {
                continue;
            };
            let raw_name: &str = dashed.as_ref();
            let name = raw_name.strip_prefix("--").unwrap_or(raw_name);
            if name.is_empty() {
                continue;
            }

            let value = decl
                .value_to_css_string(PrinterOptions {
                    minify: true,
                    ..Default::default()
                })
                .map_err(|e| format!("serialize value for --{name}: {e}"))?;

            // Trim leading whitespace — lightningcss re-emits `: value` for
            // custom properties, and `value_to_css_string` can surface an
            // extra leading space depending on the token stream.
            let trimmed = value.trim().to_string();
            out.insert(name.to_string(), trimmed);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_slot() -> PendingSlot {
        Arc::new(Mutex::new(None))
    }

    #[test]
    fn extract_empty_css_yields_empty_map() {
        let vars = extract_custom_properties(b"").expect("empty css parses");
        assert!(vars.is_empty());
    }

    #[test]
    fn extract_root_with_single_var() {
        // lightningcss minifies color tokens to their shortest form. `#ff0000`
        // collapses to `red` (3 chars < `#f00` at 4), so the extracted value
        // is the keyword, not the hex. The downstream `setProperty` call in
        // the bootstrap is insensitive to which representation it receives.
        let css = b":root { --accent: #ff0000; }";
        let vars = extract_custom_properties(css).expect("parse ok");
        assert_eq!(vars.get("accent").map(String::as_str), Some("red"));
    }

    #[test]
    fn extract_root_with_multiple_vars() {
        let css = b":root { --a: 1px; --b: 2px; --c: red; }";
        let vars = extract_custom_properties(css).expect("parse ok");
        assert_eq!(vars.get("a").map(String::as_str), Some("1px"));
        assert_eq!(vars.get("b").map(String::as_str), Some("2px"));
        assert_eq!(vars.get("c").map(String::as_str), Some("red"));
    }

    #[test]
    fn extract_ignores_non_root_selectors() {
        let css = b".card { --nope: ignored; } :root { --yes: 1; }";
        let vars = extract_custom_properties(css).expect("parse ok");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars.get("yes").map(String::as_str), Some("1"));
    }

    #[test]
    fn extract_ignores_non_custom_declarations_inside_root() {
        // Ordinary properties on :root are valid CSS (define defaults for the
        // document root element) but the bootstrap seam only forwards custom
        // properties to setProperty. Non-custom declarations are silently
        // dropped. `blue` stays as the keyword because lightningcss's minifier
        // picks whichever of `blue`/`#00f` is shorter (ties break toward the
        // keyword here).
        let css = b":root { color: red; --accent: blue; font-size: 16px; }";
        let vars = extract_custom_properties(css).expect("parse ok");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars.get("accent").map(String::as_str), Some("blue"));
    }

    #[test]
    fn extract_rejects_invalid_utf8() {
        let bad = [0xff, 0xfe, 0xfd];
        let err = extract_custom_properties(&bad).expect_err("invalid utf-8 must error");
        assert!(err.contains("invalid UTF-8"), "error was: {err}");
    }

    #[test]
    fn extract_rejects_malformed_css() {
        let bad = b":root { --a: ; ; !@# garbage!@# ";
        let err = extract_custom_properties(bad).expect_err("malformed CSS must error");
        assert!(err.contains("CSS parse failed"), "error was: {err}");
    }

    #[test]
    fn extract_rejects_compound_root_selector() {
        // `:root:hover` is deliberately skipped — see the selector filter
        // rationale in `extract_custom_properties`.
        let css = b":root:hover { --hover-only: 1; } :root { --always: 2; }";
        let vars = extract_custom_properties(css).expect("parse ok");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars.get("always").map(String::as_str), Some("2"));
        assert!(!vars.contains_key("hover-only"));
    }

    #[test]
    fn extract_captures_important_declarations() {
        let css = b":root { --override: green !important; }";
        let vars = extract_custom_properties(css).expect("parse ok");
        assert_eq!(vars.get("override").map(String::as_str), Some("green"));
    }

    #[test]
    fn snapshot_returns_baseline_bytes() {
        let slot = make_slot();
        let baseline = b":root{--x:1;}".to_vec();
        let swap = ThemeSwapImpl::new(baseline.clone(), slot);
        assert_eq!(swap.snapshot(), baseline);
    }

    #[test]
    fn apply_populates_pending_slot() {
        let slot = make_slot();
        let swap = ThemeSwapImpl::new(Vec::new(), slot.clone());
        swap.apply(b":root { --accent: #ff0000; }")
            .expect("apply ok");

        let guard = slot.lock().unwrap();
        let pending = guard.as_ref().expect("slot must be populated");
        // `#ff0000` minifies to `red` (see `extract_root_with_single_var`).
        assert_eq!(pending.vars.get("accent").map(String::as_str), Some("red"));
    }

    #[test]
    fn apply_errors_do_not_touch_slot() {
        let slot = make_slot();
        let swap = ThemeSwapImpl::new(Vec::new(), slot.clone());
        let err = swap.apply(&[0xff, 0xfe]).expect_err("bad utf8 must error");
        assert!(err.contains("invalid UTF-8"));
        assert!(slot.lock().unwrap().is_none());
    }

    #[test]
    fn revert_re_applies_baseline_bytes_via_apply() {
        let slot = make_slot();
        // Apply a candidate first.
        let candidate = b":root{--a:1;}";
        let baseline = b":root{--a:99;--b:baseline;}".to_vec();
        let swap = ThemeSwapImpl::new(baseline.clone(), slot.clone());
        swap.apply(candidate).expect("apply candidate");
        // Drain to simulate a frame.
        let _js = drain_pending_js(&slot).expect("pending after apply");

        // Revert with the snapshot bytes.
        let snap = swap.snapshot();
        swap.revert(&snap).expect("revert ok");

        let guard = slot.lock().unwrap();
        let pending = guard.as_ref().expect("revert repopulates slot");
        assert_eq!(pending.vars.get("a").map(String::as_str), Some("99"));
        assert_eq!(
            pending.vars.get("b").map(String::as_str),
            Some("baseline")
        );
    }

    #[test]
    fn drain_empty_slot_returns_none() {
        let slot = make_slot();
        assert!(drain_pending_js(&slot).is_none());
    }

    #[test]
    fn drain_populated_slot_emits_set_theme_invocation() {
        let slot = make_slot();
        {
            let mut g = slot.lock().unwrap();
            let mut vars = HashMap::new();
            vars.insert("accent".to_string(), "#ff0000".to_string());
            *g = Some(PendingTheme { vars });
        }
        let js = drain_pending_js(&slot).expect("pending populated");
        assert!(js.contains("window.__omni_set_theme"));
        assert!(js.contains("\"accent\""));
        assert!(js.contains("\"#ff0000\""));
        // Drain consumes the slot — next drain is None.
        assert!(drain_pending_js(&slot).is_none());
    }

    #[test]
    fn apply_then_drain_then_revert_then_drain_roundtrip() {
        let slot = make_slot();
        let baseline = b":root{--theme:dark;}".to_vec();
        let swap = ThemeSwapImpl::new(baseline.clone(), slot.clone());

        // Candidate apply → drain produces a set_theme call.
        swap.apply(b":root{--theme:light;}").expect("apply");
        let js = drain_pending_js(&slot).expect("apply populated slot");
        assert!(js.contains("\"theme\""));
        assert!(js.contains("\"light\""));

        // Revert with the baseline snapshot → drain produces another call.
        let snap = swap.snapshot();
        swap.revert(&snap).expect("revert");
        let js2 = drain_pending_js(&slot).expect("revert populated slot");
        assert!(js2.contains("\"theme\""));
        assert!(js2.contains("\"dark\""));

        // Nothing pending after both drains.
        assert!(drain_pending_js(&slot).is_none());
    }
}
