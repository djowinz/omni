//! Integration smoke tests for scoped Ultralight FS + trust filter.
//!
//! These tests require the Ultralight DLLs to be present on the DLL search
//! path (same arrangement as `cargo run`). They are gated by the
//! `ULTRALIGHT_SMOKE` env var so CI that lacks the runtime can skip them.

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, Once};

use omni_host::omni::view_trust::ViewTrust;
use omni_host::ul_renderer::UlRenderer;

// Ultralight is process-global; tests must not run two renderers concurrently.
static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

static LOG_INIT: Once = Once::new();
fn init_tracing() {
    LOG_INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
            )
            .with_test_writer()
            .try_init();
    });
}

fn have_ul() -> bool { std::env::var("ULTRALIGHT_SMOKE").is_ok() }

fn resources_dir() -> PathBuf {
    if let Ok(p) = std::env::var("OMNI_UL_RESOURCES") {
        return PathBuf::from(p);
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn tmp_overlay(tag: &str) -> PathBuf {
    let id = std::process::id();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    let dir = std::env::temp_dir().join(format!("omni_smoke_{id}_{tag}_{stamp}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn non_zero_pixels(ul: &UlRenderer) -> usize {
    let count = std::cell::Cell::new(0usize);
    ul.with_pixels(|_, _, _, pixels, _| {
        let n = pixels.chunks_exact(4).filter(|px| px[3] != 0).count();
        count.set(n);
    });
    count.get()
}

fn pump(ul: &UlRenderer, frames: u32) {
    for _ in 0..frames {
        ul.update_and_render();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

#[test]
fn custom_font_and_image_resolve() {
    if !have_ul() { return; }
    let _g = GLOBAL_LOCK.lock().unwrap();
    init_tracing();

    let overlay = tmp_overlay("font_image");
    fs::create_dir_all(overlay.join("fonts")).unwrap();
    fs::create_dir_all(overlay.join("images")).unwrap();
    let png_1x1: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00\x1f\x15\xc4\x89\x00\x00\x00\rIDATx\x9cc\xfa\xcf\xc0\x00\x00\x00\x03\x00\x01\x5b\x13\x3d\x1a\x00\x00\x00\x00IEND\xaeB`\x82";
    fs::write(overlay.join("images/dot.png"), png_1x1).unwrap();

    let html = r#"<!doctype html><html><head><style>
        body { margin:0; background: #0088ff; color: white; font-size: 48px; }
    </style></head><body>
        <h1>SMOKE</h1>
        <img src="images/dot.png" style="width:100px;height:100px;"/>
    </body></html>"#;

    let ul = UlRenderer::init(400, 200, &resources_dir())
        .expect("Ultralight init failed — ensure DLLs are on the path");
    ul.mount(&overlay, html, ViewTrust::LocalAuthored).expect("mount failed");
    pump(&ul, 20);

    let non_zero = non_zero_pixels(&ul);
    assert!(non_zero > 1000, "expected rendered pixels, got {non_zero}");

    fs::remove_dir_all(&overlay).ok();
}

#[test]
fn parent_escape_request_is_rejected() {
    if !have_ul() { return; }
    let _g = GLOBAL_LOCK.lock().unwrap();
    init_tracing();

    let overlay = tmp_overlay("escape");
    let outside = tmp_overlay("escape_outside");
    fs::write(outside.join("secret.png"), b"\x89PNG\r\n").unwrap();

    let html = r#"<!doctype html><html><body style="background:#111;">
        <img src="../omni_smoke_escape_outside_fake/secret.png"/>
    </body></html>"#;

    let ul = UlRenderer::init(200, 100, &resources_dir()).expect("Ultralight init failed");
    ul.mount(&overlay, html, ViewTrust::LocalAuthored).expect("mount");
    pump(&ul, 10);

    let _ = non_zero_pixels(&ul);

    fs::remove_dir_all(&overlay).ok();
    fs::remove_dir_all(&outside).ok();
}

#[test]
fn bundle_installed_trust_rejects_http() {
    if !have_ul() { return; }
    let _g = GLOBAL_LOCK.lock().unwrap();
    init_tracing();

    let overlay = tmp_overlay("http_reject");
    let html = r#"<!doctype html><html><body style="background:#222;">
        <img src="http://127.0.0.1:9/nope.png"/>
    </body></html>"#;

    let ul = UlRenderer::init(200, 100, &resources_dir()).expect("Ultralight init failed");
    ul.mount(&overlay, html, ViewTrust::BundleInstalled).expect("mount");
    pump(&ul, 10);

    let _ = non_zero_pixels(&ul);

    fs::remove_dir_all(&overlay).ok();
}
