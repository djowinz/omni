//! Integration test for `UlRenderer::recreate_view` across device scales.
//!
//! Constructs a renderer at scale=None@1920x1080, recreates at
//! scale=Some(2.0)@3840x2160, mounts a fixture overlay between phases, and
//! asserts the surface dimensions match plus non-empty pixel data after the
//! 2.0 phase. Then cycles back to scale=None@1920x1080 and confirms no crash.
//!
//! This exercises architectural invariant #24 (fs_dispatcher mount-ordering):
//! `recreate_view` must drop the mount handle BEFORE destroying the view, and
//! the caller is responsible for re-mounting afterward. If this test crashes
//! with `STATUS_STACK_BUFFER_OVERRUN` or an access violation, that is a strong
//! signal that the mount-handle drop ordering regressed.
//!
//! Gated by the `ULTRALIGHT_SMOKE` env var (mirrors the established pattern in
//! `overlay_sandbox_smoke.rs`); skips cleanly when Ultralight DLLs aren't on
//! the search path. When `ULTRALIGHT_SMOKE=1` is set, `OMNI_UL_RESOURCES` must
//! point at the Ultralight resources directory.
//!
//! Spec: docs/superpowers/specs/2026-04-25-overlay-dpi-scale-design.md

use std::path::PathBuf;
use std::sync::Mutex;

use omni_host::omni::view_trust::ViewTrust;
use omni_host::ul_renderer::UlRenderer;
use tempfile::TempDir;

// Ultralight is process-global; tests must not run two renderers concurrently.
// Mirrors the GLOBAL_LOCK in `overlay_sandbox_smoke.rs`. Although each
// integration-test binary runs in its own process, keeping this here means a
// future test added to this file inherits the same serialization guarantee.
static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

fn have_ul() -> bool {
    std::env::var("ULTRALIGHT_SMOKE").is_ok()
}

fn resources_dir() -> PathBuf {
    std::env::var("OMNI_UL_RESOURCES")
        .map(PathBuf::from)
        .expect(
            "OMNI_UL_RESOURCES must point at the Ultralight resources directory when ULTRALIGHT_SMOKE=1",
        )
}

fn skip_msg(name: &str) {
    eprintln!("skipped {name}: set ULTRALIGHT_SMOKE=1 and OMNI_UL_RESOURCES=... to run");
}

const FIXTURE_HTML: &str = "<!doctype html><html><body style=\"margin:0;background:transparent\">\
    <div id=\"w\" style=\"position:fixed;left:50%;margin-left:-150px;top:50%;margin-top:-25px;\
    width:300px;height:50px;background:red\"></div></body></html>";

#[test]
fn recreate_view_across_scales_renders_cleanly() {
    if !have_ul() {
        skip_msg(module_path!());
        return;
    }
    let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let mut ul = UlRenderer::init(1920, 1080, None, &resources_dir())
        .expect("init at None@1920x1080");
    let tmp = TempDir::new().expect("tmpdir");

    // Phase 1: scale=None @ 1920x1080.
    ul.mount(tmp.path(), FIXTURE_HTML, ViewTrust::LocalAuthored)
        .expect("mount initial");
    for _ in 0..3 {
        ul.update_and_render();
    }

    // Phase 2: recreate at scale=Some(2.0) @ 3840x2160.
    ul.recreate_view(3840, 2160, Some(2.0))
        .expect("recreate at Some(2.0)@3840x2160");
    ul.mount(tmp.path(), FIXTURE_HTML, ViewTrust::LocalAuthored)
        .expect("mount after first recreation");
    for _ in 0..3 {
        ul.update_and_render();
    }

    // Surface should reflect the new dims and produce non-empty pixels.
    let mut got_pixels = false;
    let mut surface_w = 0u32;
    let mut surface_h = 0u32;
    ul.with_pixels(|w, h, _row_bytes, pixels, _dirty| {
        surface_w = w;
        surface_h = h;
        if pixels.iter().any(|&b| b != 0) {
            got_pixels = true;
        }
    });
    assert_eq!(
        surface_w, 3840,
        "surface width should be 3840 after recreate at Some(2.0)@3840x2160"
    );
    assert_eq!(
        surface_h, 2160,
        "surface height should be 2160 after recreate at Some(2.0)@3840x2160"
    );
    assert!(
        got_pixels,
        "surface was empty after recreation at scale=Some(2.0)@3840x2160"
    );

    // Phase 3: cycle back to scale=None @ 1920x1080. Confirm no crash.
    ul.recreate_view(1920, 1080, None)
        .expect("recreate back to None@1920x1080");
    ul.mount(tmp.path(), FIXTURE_HTML, ViewTrust::LocalAuthored)
        .expect("mount after second recreation");
    for _ in 0..3 {
        ul.update_and_render();
    }
}
