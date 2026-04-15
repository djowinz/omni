//! End-to-end smoke test: real Ultralight + privileged bootstrap.
//!
//! Loads documents produced by `build_initial_html`, pushes sensor updates
//! through `__omni_update`, and reads back DOM state via
//! `evaluate_script_result`. Verifies contract §5 (target semantics), §6
//! (format table), threshold-class toggling, and the untrusted-view defang.
//!
//! Runs a single #[test] because Ultralight's platform handlers are
//! process-global and UlRenderer holds raw pointers (!Send).

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use omni_host::omni::default::DEFAULT_OMNI;
use omni_host::omni::history::SensorHistory;
use omni_host::omni::html_builder::{build_initial_html, format_values_js};
use omni_host::omni::js_bootstrap::render_script_tag;
use omni_host::omni::parser::parse_omni_with_diagnostics;
use omni_host::omni::view_trust::ViewTrust;
use omni_host::ul_renderer::UlRenderer;
use omni_shared::SensorSnapshot;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn pump(ul: &UlRenderer, frames: usize) {
    for _ in 0..frames {
        ul.update_and_render();
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// Scratch directory reused across all `load` calls within the test.
/// A single dir is fine — mount() overwrites the scratch file each call.
static SCRATCH_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

fn scratch_dir() -> &'static std::path::PathBuf {
    SCRATCH_DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join("omni_bootstrap_ul_smoke");
        std::fs::create_dir_all(&dir).ok();
        dir
    })
}

/// Load a fresh document into the View and pump enough frames for the
/// bootstrap's DOMContentLoaded to run.
fn load(ul: &UlRenderer, doc: &str) {
    ul.mount(scratch_dir(), doc, ViewTrust::LocalAuthored)
        .expect("mount failed in smoke test load()");
    pump(ul, 25);
}

/// Build a full HTML document with bootstrap + a hand-crafted span.
/// `trust` controls whether the bootstrap defangs APIs.
fn make_custom_doc(
    path: &str,
    format: &str,
    precision: usize,
    extra_attrs: &[(&str, &str)],
    trust: ViewTrust,
) -> String {
    let bootstrap = render_script_tag(trust);
    let mut extra = String::new();
    for (k, v) in extra_attrs {
        extra.push_str(&format!(r#" {}="{}""#, k, v));
    }
    format!(
        r#"<!DOCTYPE html>
<html>
<head><meta charset="utf-8">{bootstrap}</head>
<body>
<span data-sensor="{path}" data-sensor-format="{format}" data-sensor-precision="{precision}"{extra}>initial</span>
</body>
</html>"#,
        bootstrap = bootstrap,
        path = path,
        format = format,
        precision = precision,
        extra = extra,
    )
}

/// Read element textContent via evaluate_script_result.
fn assert_text(
    ul: &UlRenderer,
    label: &str,
    selector: &str,
    expected: &str,
    failures: &mut Vec<String>,
) {
    let js = format!(
        "(function(){{ var el = document.querySelector({sel:?}); return el ? el.textContent : '__missing__'; }})()",
        sel = selector
    );
    match ul.evaluate_script_result(&js) {
        Ok(v) if v == expected => {}
        Ok(v) => failures.push(format!("{label}: expected {expected:?}, got {v:?}")),
        Err(e) => failures.push(format!("{label}: JS exception — {e}")),
    }
}

/// Evaluate JS and collect result or exception as String.
fn eval(ul: &UlRenderer, js: &str) -> String {
    match ul.evaluate_script_result(js) {
        Ok(v) => v,
        Err(e) => format!("__exception__:{e}"),
    }
}

/// Push values via __omni_update and pump a few frames so the DOM reflects them.
fn push_update(ul: &UlRenderer, values: &HashMap<String, f64>) {
    let js = format_values_js(values);
    ul.evaluate_script(&js);
    pump(ul, 3);
}

// ---------------------------------------------------------------------------
// Group A — default overlay E2E
// ---------------------------------------------------------------------------

fn run_group_a(ul: &UlRenderer, failures: &mut Vec<String>) {
    let (file_opt, diags) = parse_omni_with_diagnostics(DEFAULT_OMNI);
    let file = file_opt.unwrap_or_else(|| panic!("parse DEFAULT_OMNI: {:?}", diags));
    let snap = SensorSnapshot::default();
    let hv: HashMap<String, f64> = HashMap::new();
    let hu: HashMap<String, String> = HashMap::new();
    let history = SensorHistory::new();

    let rendered = build_initial_html(
        &file,
        &snap,
        1920,
        1080,
        Path::new("."),
        "default",
        &hv,
        &hu,
        &history,
        ViewTrust::LocalAuthored,
    );

    load(ul, &rendered.full_document);

    // Verify sensors are present (scan ran after DOMContentLoaded).
    // If selectors return __missing__, the scan didn't run — that's a real failure.
    let check_exists = eval(
        ul,
        r#"(function(){ var el = document.querySelector('[data-sensor="cpu.usage"]'); return el ? 'found' : 'missing'; })()"#,
    );
    if check_exists != "found" {
        failures.push(
            "A.sanity: data-sensor='cpu.usage' element missing from DOM after load — scan may not have run".to_string()
        );
        // If the element is missing, subsequent selectors will all fail; push a
        // placeholder so the report is obvious about the root cause.
        failures.push(
            "A: all sub-assertions skipped because sensor spans were not found in DOM".into(),
        );
        return;
    }

    // Push sensor values.
    let mut values: HashMap<String, f64> = HashMap::new();
    values.insert("cpu.usage".into(), 42.0);
    values.insert("cpu.temp".into(), 55.0);
    values.insert("gpu.usage".into(), 88.0);
    values.insert("gpu.temp".into(), 71.0);
    values.insert("gpu.clock".into(), 1750.0);
    values.insert("gpu.vram.used".into(), 4096.0);
    values.insert("gpu.vram.total".into(), 12288.0);
    values.insert("gpu.power".into(), 180.0);
    values.insert("gpu.fan".into(), 60.0);
    values.insert("ram.usage".into(), 45.0);
    values.insert("fps".into(), 144.0);
    push_update(ul, &values);

    // cpu.usage: percent format, precision 0, value 42 → "42%"
    assert_text(
        ul,
        "A.cpu.usage",
        r#"[data-sensor="cpu.usage"]"#,
        "42%",
        failures,
    );

    // cpu.temp: temperature format, precision 0, value 55 → "55°C"
    assert_text(
        ul,
        "A.cpu.temp",
        r#"[data-sensor="cpu.temp"]"#,
        "55\u{00B0}C",
        failures,
    );

    // gpu.fan: percent format, precision 0, value 60 → "60%"
    assert_text(
        ul,
        "A.gpu.fan",
        r#"[data-sensor="gpu.fan"]"#,
        "60%",
        failures,
    );

    // fps: raw format, precision 0, value 144 → "144"
    assert_text(ul, "A.fps", r#"[data-sensor="fps"]"#, "144", failures);
}

// ---------------------------------------------------------------------------
// Group B — contract §6 format table
// ---------------------------------------------------------------------------

fn run_group_b(ul: &UlRenderer, failures: &mut Vec<String>) {
    // Helper: load a custom doc with one span, push a value, read textContent.
    let mut check = |label: &str,
                     path: &str,
                     format: &str,
                     precision: usize,
                     raw_value: f64,
                     expected: &str| {
        let doc = make_custom_doc(path, format, precision, &[], ViewTrust::LocalAuthored);
        load(ul, &doc);

        let mut values: HashMap<String, f64> = HashMap::new();
        values.insert(path.to_string(), raw_value);
        push_update(ul, &values);

        let js = format!(
            r#"(function(){{ var el = document.querySelector('[data-sensor="{path}"]'); return el ? el.textContent : '__missing__'; }})()"#,
        );
        match ul.evaluate_script_result(&js) {
            Ok(v) if v == expected => {}
            Ok(v) => failures.push(format!("{label}: expected {expected:?}, got {v:?}")),
            Err(e) => failures.push(format!("{label}: JS exception — {e}")),
        }
    };

    // percent — value ≤ 1 is scaled × 100, value > 1 used as-is.
    // 0.73 ≤ 1 → 0.73 × 100 = 73 → "73%"
    check("B.percent.0.73", "sensor.a", "percent", 0, 0.73, "73%");

    // 42 > 1 → "42%"
    check("B.percent.42", "sensor.b", "percent", 0, 42.0, "42%");

    // temperature precision 1: 71.5 → "71.5°C"
    check(
        "B.temperature.1",
        "sensor.c",
        "temperature",
        1,
        71.5,
        "71.5\u{00B0}C",
    );

    // bytes precision 0: 1536 bytes → 1536/1024 = 1.5 KB → toFixed(0) → "2 KB"
    // JS Number.prototype.toFixed(0) on 1.5: JS uses half-away-from-zero for
    // positive values, so 1.5.toFixed(0) === "2".
    check("B.bytes.0", "sensor.d", "bytes", 0, 1536.0, "2 KB");

    // bytes precision 1: 1536 → 1.5.toFixed(1) → "1.5 KB"
    check("B.bytes.1", "sensor.e", "bytes", 1, 1536.0, "1.5 KB");

    // bytes precision 2: 1_572_864 bytes → 1572864/1024/1024 = 1.5 MB → "1.50 MB"
    check("B.bytes.2", "sensor.f", "bytes", 2, 1_572_864.0, "1.50 MB");

    // frequency precision 2: 3_500_000_000 Hz → 3.5 GHz → toFixed(2) → "3.50 GHz"
    check(
        "B.frequency.ghz",
        "sensor.g",
        "frequency",
        2,
        3_500_000_000.0,
        "3.50 GHz",
    );

    // frequency precision 0: 3_500_000 Hz → 3.5 MHz → toFixed(0)
    // JS toFixed(0) of 3.5: half-away-from-zero → "4" (positive)
    // NOTE: V8-compatible engines may produce "4"; record actual if it differs.
    check(
        "B.frequency.mhz",
        "sensor.h",
        "frequency",
        0,
        3_500_000.0,
        "4 MHz",
    );

    // raw precision 1: 42.456 → "42.5"
    check("B.raw.1", "sensor.i", "raw", 1, 42.456, "42.5");
}

// ---------------------------------------------------------------------------
// Group C — §5 target semantics
// ---------------------------------------------------------------------------

fn run_group_c(ul: &UlRenderer, failures: &mut Vec<String>) {
    let bootstrap = render_script_tag(ViewTrust::LocalAuthored);

    // ---- C1: target="text" (baseline, already covered by Group A/B) --------
    {
        let doc = make_custom_doc("sensor.text", "raw", 0, &[], ViewTrust::LocalAuthored);
        load(ul, &doc);
        let mut v: HashMap<String, f64> = HashMap::new();
        v.insert("sensor.text".into(), 77.0);
        push_update(ul, &v);
        assert_text(
            ul,
            "C1.text_default",
            r#"[data-sensor="sensor.text"]"#,
            "77",
            failures,
        );
    }

    // ---- C2: target="attr:value" on an <output> element --------------------
    {
        let doc = format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">{bootstrap}</head>
<body>
<output data-sensor="sensor.attrval"
        data-sensor-format="raw"
        data-sensor-precision="1"
        data-sensor-target="attr:value">initial</output>
</body></html>"#,
        );
        load(ul, &doc);
        let mut v: HashMap<String, f64> = HashMap::new();
        v.insert("sensor.attrval".into(), 88.8);
        push_update(ul, &v);

        let js = r#"(function(){
            var el = document.querySelector('[data-sensor="sensor.attrval"]');
            if (!el) return '__missing__';
            return el.getAttribute('value') || '__no_attr__';
        })()"#;
        match ul.evaluate_script_result(js) {
            Ok(v) if v == "88.8" => {}
            Ok(v) => failures.push(format!("C2.attr_value: expected \"88.8\", got {v:?}")),
            Err(e) => failures.push(format!("C2.attr_value: JS exception — {e}")),
        }
    }

    // ---- C3: target="style-var:accent" on documentElement -----------------
    {
        let doc = format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">{bootstrap}</head>
<body>
<span data-sensor="sensor.stylevar"
      data-sensor-format="raw"
      data-sensor-precision="0"
      data-sensor-target="style-var:accent">initial</span>
</body></html>"#,
        );
        load(ul, &doc);
        let mut v: HashMap<String, f64> = HashMap::new();
        v.insert("sensor.stylevar".into(), 99.0);
        push_update(ul, &v);

        // The bootstrap sets `el.style.setProperty("--accent", formatted)` on
        // the element itself (not documentElement). But per spec it operates on
        // the element that carries the data-sensor attr.
        let js = r#"(function(){
            var el = document.querySelector('[data-sensor="sensor.stylevar"]');
            if (!el) return '__missing__';
            return el.style.getPropertyValue('--accent');
        })()"#;
        match ul.evaluate_script_result(js) {
            Ok(v) if v.contains("99") => {}
            Ok(v) => failures.push(format!(
                "C3.style_var_accent: expected value containing '99', got {v:?}"
            )),
            Err(e) => failures.push(format!("C3.style_var_accent: JS exception — {e}")),
        }
    }

    // ---- C4: target="class" — bootstrap sanitizes with CLASS_RE, sets className
    {
        let doc = format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">{bootstrap}</head>
<body>
<span data-sensor="sensor.cls"
      data-sensor-format="raw"
      data-sensor-precision="0"
      data-sensor-target="class">initial</span>
</body></html>"#,
        );
        load(ul, &doc);
        let mut v: HashMap<String, f64> = HashMap::new();
        // Push integer 5 → format "5" → matches CLASS_RE → className set.
        v.insert("sensor.cls".into(), 5.0);
        push_update(ul, &v);

        let js = r#"(function(){
            var el = document.querySelector('[data-sensor="sensor.cls"]');
            if (!el) return '__missing__';
            return el.className;
        })()"#;
        match ul.evaluate_script_result(js) {
            Ok(v) if v == "5" => {}
            Ok(v) => failures.push(format!("C4.target_class: expected \"5\", got {v:?}")),
            Err(e) => failures.push(format!("C4.target_class: JS exception — {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Group D — threshold classes
// ---------------------------------------------------------------------------

fn run_group_d(ul: &UlRenderer, failures: &mut Vec<String>) {
    let bootstrap = render_script_tag(ViewTrust::LocalAuthored);

    // ---- D1: warn-only threshold (no critical) ------------------------------
    {
        let doc = format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">{bootstrap}</head>
<body>
<span id="twarn"
      data-sensor="sensor.warn"
      data-sensor-format="raw"
      data-sensor-precision="0"
      data-sensor-threshold-warn="80">0</span>
</body></html>"#,
        );
        load(ul, &doc);

        // Push 50 — below warn threshold.
        let mut v: HashMap<String, f64> = HashMap::new();
        v.insert("sensor.warn".into(), 50.0);
        push_update(ul, &v);
        let js = r#"(function(){
            var el = document.getElementById('twarn');
            return el ? String(el.classList.contains('sensor-warn')) : '__missing__';
        })()"#;
        match ul.evaluate_script_result(js) {
            Ok(v) if v == "false" => {}
            Ok(v) => failures.push(format!(
                "D1a.warn_below: expected 'false' (sensor-warn absent), got {v:?}"
            )),
            Err(e) => failures.push(format!("D1a.warn_below: JS exception — {e}")),
        }

        // Push 85 — above warn threshold.
        v.insert("sensor.warn".into(), 85.0);
        push_update(ul, &v);
        let js2 = r#"(function(){
            var el = document.getElementById('twarn');
            return el ? String(el.classList.contains('sensor-warn')) : '__missing__';
        })()"#;
        match ul.evaluate_script_result(js2) {
            Ok(v) if v == "true" => {}
            Ok(v) => failures.push(format!(
                "D1b.warn_above: expected 'true' (sensor-warn present), got {v:?}"
            )),
            Err(e) => failures.push(format!("D1b.warn_above: JS exception — {e}")),
        }
    }

    // ---- D2: both warn and critical — supersede semantics ------------------
    {
        let doc = format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">{bootstrap}</head>
<body>
<span id="tcrit"
      data-sensor="sensor.crit"
      data-sensor-format="raw"
      data-sensor-precision="0"
      data-sensor-threshold-warn="80"
      data-sensor-threshold-critical="95">0</span>
</body></html>"#,
        );
        load(ul, &doc);

        // Push 90 — above warn, below critical → sensor-warn=true, sensor-critical=false.
        let mut v: HashMap<String, f64> = HashMap::new();
        v.insert("sensor.crit".into(), 90.0);
        push_update(ul, &v);
        let js_warn = r#"(function(){
            var el = document.getElementById('tcrit');
            if (!el) return '__missing__';
            return el.classList.contains('sensor-warn') + ':' + el.classList.contains('sensor-critical');
        })()"#;
        match ul.evaluate_script_result(js_warn) {
            Ok(v) if v == "true:false" => {}
            Ok(v) => failures.push(format!(
                "D2a.warn_not_crit: expected 'true:false', got {v:?}"
            )),
            Err(e) => failures.push(format!("D2a.warn_not_crit: JS exception — {e}")),
        }

        // Push 97 — above critical → sensor-critical=true, sensor-warn=false (supersedes).
        v.insert("sensor.crit".into(), 97.0);
        push_update(ul, &v);
        let js_crit = r#"(function(){
            var el = document.getElementById('tcrit');
            if (!el) return '__missing__';
            return el.classList.contains('sensor-warn') + ':' + el.classList.contains('sensor-critical');
        })()"#;
        match ul.evaluate_script_result(js_crit) {
            Ok(v) if v == "false:true" => {}
            Ok(v) => failures.push(format!(
                "D2b.crit_supersedes_warn: expected 'false:true', got {v:?}"
            )),
            Err(e) => failures.push(format!("D2b.crit_supersedes_warn: JS exception — {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// Group E — untrusted view defang
// ---------------------------------------------------------------------------

fn run_group_e(ul: &UlRenderer, failures: &mut Vec<String>) {
    let doc = make_custom_doc("sensor.e", "raw", 0, &[], ViewTrust::BundleInstalled);
    load(ul, &doc);

    // E1: fetch should be undefined
    match ul.evaluate_script_result("typeof window.fetch") {
        Ok(v) if v == "undefined" => {}
        Ok(v) => failures.push(format!(
            "E1.fetch_defanged: expected 'undefined', got {v:?}"
        )),
        Err(e) => failures.push(format!("E1.fetch_defanged: JS exception — {e}")),
    }

    // E2: XMLHttpRequest should be undefined
    match ul.evaluate_script_result("typeof window.XMLHttpRequest") {
        Ok(v) if v == "undefined" => {}
        Ok(v) => failures.push(format!("E2.xhr_defanged: expected 'undefined', got {v:?}")),
        Err(e) => failures.push(format!("E2.xhr_defanged: JS exception — {e}")),
    }

    // E3: WebSocket should be undefined
    match ul.evaluate_script_result("typeof window.WebSocket") {
        Ok(v) if v == "undefined" => {}
        Ok(v) => failures.push(format!(
            "E3.websocket_defanged: expected 'undefined', got {v:?}"
        )),
        Err(e) => failures.push(format!("E3.websocket_defanged: JS exception — {e}")),
    }

    // E4: eval should be blocked (bootstrap replaces it with a throwing stub).
    let eval_check = eval(
        ul,
        r#"(function(){ try { window.eval('1+1'); return 'allowed'; } catch(e) { return 'blocked'; } })()"#,
    );
    if eval_check != "blocked" {
        failures.push(format!(
            "E4.eval_blocked: expected 'blocked', got {eval_check:?}"
        ));
    }

    // E5: KNOWN BYPASS — Function constructor reachable via prototype chain.
    // ({}).constructor.constructor is Object's constructor's constructor = Function.
    // The bootstrap stubs window.Function but cannot patch Function.prototype or
    // the intrinsic [[Construct]] of existing objects. This is a known limitation.
    // We assert the bypass EXISTS (current reality) so the test fails loudly the
    // day someone fixes it, prompting a review of whether it's truly closed.
    let bypass_typeof = eval(ul, "typeof ({}).constructor.constructor");
    if bypass_typeof != "function" {
        // Unexpectedly the bypass is closed — surface as informational.
        failures.push(format!(
            "KNOWN_BYPASS: ({{}}).constructor.constructor typeof = {bypass_typeof:?} — expected 'function' (encodes known bypass state; update test if bypass is intentionally closed)"
        ));
    }
    // No else — if it IS "function" we silently accept (bypass known, not fixed).

    // E6: localStorage should be undefined
    match ul.evaluate_script_result("typeof window.localStorage") {
        Ok(v) if v == "undefined" => {}
        Ok(v) => failures.push(format!(
            "E6.localstorage_defanged: expected 'undefined', got {v:?}"
        )),
        Err(e) => failures.push(format!("E6.localstorage_defanged: JS exception — {e}")),
    }

    // E7: window.Function stub — typeof is "function" (stub is a function object),
    // but calling it should throw.
    // First confirm typeof is still "function" (stub was installed).
    match ul.evaluate_script_result("typeof window.Function") {
        Ok(v) if v == "function" => {}
        Ok(v) => failures.push(format!(
            "E7a.Function_typeof: expected 'function' (stub installed), got {v:?}"
        )),
        Err(e) => failures.push(format!("E7a.Function_typeof: JS exception — {e}")),
    }

    // Calling it should throw.
    let fn_call = eval(
        ul,
        r#"(function(){ try { window.Function('return 1'); return 'allowed'; } catch(e) { return 'blocked'; } })()"#,
    );
    if fn_call != "blocked" {
        failures.push(format!(
            "E7b.Function_blocked: expected 'blocked', got {fn_call:?}"
        ));
    }
}

// ---------------------------------------------------------------------------
// Group F — trusted invariants
// ---------------------------------------------------------------------------

fn run_group_f(ul: &UlRenderer, failures: &mut Vec<String>) {
    let doc = make_custom_doc("sensor.f", "raw", 0, &[], ViewTrust::LocalAuthored);
    load(ul, &doc);

    // F1: fetch should be a function (trusted views NOT defanged).
    match ul.evaluate_script_result("typeof window.fetch") {
        Ok(v) if v == "function" => {}
        Ok(v) => failures.push(format!("F1.fetch_trusted: expected 'function', got {v:?}")),
        Err(e) => failures.push(format!("F1.fetch_trusted: JS exception — {e}")),
    }

    // F2: eval should work normally in a trusted view.
    match ul.evaluate_script_result(r#"eval("1+2").toString()"#) {
        Ok(v) if v == "3" => {}
        Ok(v) => failures.push(format!("F2.eval_trusted: expected '3', got {v:?}")),
        Err(e) => failures.push(format!("F2.eval_trusted: JS exception — {e}")),
    }
}

// ---------------------------------------------------------------------------
// Single test entry point
// ---------------------------------------------------------------------------

#[test]
fn bootstrap_integration_smoke() {
    // The test binary is placed in target/debug/deps/; the resources/ dir and
    // the Ultralight DLLs are in target/debug/ (one level up). Resolve that
    // parent directory so UlRenderer can find its DLLs and resources/.
    let exe_dir = std::env::current_exe()
        .expect("current_exe")
        .parent()
        .expect("exe parent (deps/)")
        .parent()
        .expect("parent of deps/ — should be target/debug/")
        .to_path_buf();

    let ul = match UlRenderer::init(800, 400, &exe_dir) {
        Ok(r) => r,
        Err(e) => {
            panic!(
                "BLOCKED: UlRenderer::init failed — {e}\n\
                 Ensure Ultralight DLLs are in target/debug/ and \
                 resources/ (icudt67l.dat, cacert.pem) exist there."
            );
        }
    };

    let mut failures: Vec<String> = Vec::new();

    run_group_a(&ul, &mut failures);
    run_group_b(&ul, &mut failures);
    run_group_c(&ul, &mut failures);
    run_group_d(&ul, &mut failures);
    run_group_e(&ul, &mut failures);
    run_group_f(&ul, &mut failures);

    if !failures.is_empty() {
        panic!(
            "bootstrap integration smoke had {} failure(s):\n{}",
            failures.len(),
            failures.join("\n")
        );
    }
}
