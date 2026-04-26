//! Integration smoke tests for scoped Ultralight FS + trust filter.
//!
//! These tests require the Ultralight DLLs to be present on the DLL search
//! path (same arrangement as `cargo run`). They are gated by the
//! `ULTRALIGHT_SMOKE` env var so CI that lacks the runtime can skip them.
//!
//! When `ULTRALIGHT_SMOKE=1` is set, `OMNI_UL_RESOURCES` is also required and
//! must point at the Ultralight resources directory (the one containing
//! `cacert.pem`, `icudt*.dat`, etc.) so the renderer can initialize.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Once};

use omni_host::omni::view_trust::ViewTrust;
use omni_host::ul_renderer::UlRenderer;

// Ultralight is process-global; tests must not run two renderers concurrently.
static GLOBAL_LOCK: Mutex<()> = Mutex::new(());

/// Inline solid-red 1x1 PNG (pre-verified valid bytes).
const RED_1X1_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x00, 0x03, 0x00, 0x01, 0x5B, 0x12, 0x2D, 0xAC, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

#[derive(Clone, Default)]
struct BufWriter(Arc<Mutex<Vec<u8>>>);

impl io::Write for BufWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufWriter {
    type Writer = BufWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

impl BufWriter {
    fn contents(&self) -> String {
        let guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        String::from_utf8_lossy(&guard).into_owned()
    }
    fn clear(&self) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

static LOG_INIT: Once = Once::new();
static LOG_BUF: Mutex<Option<BufWriter>> = Mutex::new(None);

fn init_tracing() -> BufWriter {
    let mut slot = LOG_BUF.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(b) = slot.as_ref() {
        return b.clone();
    }
    let buf = BufWriter::default();
    let buf_clone = buf.clone();
    LOG_INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
            )
            .with_writer(buf_clone)
            .with_ansi(false)
            .try_init();
    });
    *slot = Some(buf.clone());
    buf
}

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

struct TmpDir(PathBuf);
impl Drop for TmpDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl TmpDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

fn tmp_overlay(tag: &str) -> TmpDir {
    let id = std::process::id();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("omni_smoke_{id}_{tag}_{stamp}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    TmpDir(dir)
}

fn pump(ul: &UlRenderer, frames: u32) {
    for _ in 0..frames {
        ul.update_and_render();
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}

fn sample_center(ul: &UlRenderer) -> Option<[u8; 4]> {
    let mut out = None;
    ul.with_pixels(|w, h, row_bytes, pixels, _| {
        let x = (w / 2) as usize;
        let y = (h / 2) as usize;
        let idx = y * row_bytes as usize + x * 4;
        if idx + 3 < pixels.len() {
            out = Some([
                pixels[idx],
                pixels[idx + 1],
                pixels[idx + 2],
                pixels[idx + 3],
            ]);
        }
    });
    out
}

fn skip_msg(name: &str) {
    eprintln!("skipped {name}: set ULTRALIGHT_SMOKE=1 and OMNI_UL_RESOURCES=... to run");
}

#[test]
fn custom_font_and_image_resolve() {
    if !have_ul() {
        skip_msg(module_path!());
        return;
    }
    let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let buf = init_tracing();
    buf.clear();

    let overlay = tmp_overlay("font_image");
    fs::create_dir_all(overlay.path().join("fonts")).unwrap();
    fs::create_dir_all(overlay.path().join("images")).unwrap();
    fs::write(overlay.path().join("images/red.png"), RED_1X1_PNG).unwrap();

    let html = r#"<!doctype html><html><head><style>
        html, body { margin:0; padding:0; background: transparent; }
        .img { position:fixed; inset:0; background-image: url("images/red.png");
               background-size: cover; image-rendering: pixelated; }
    </style></head><body><div class="img"></div></body></html>"#;

    let ul = UlRenderer::init(400, 200, None, &resources_dir())
        .expect("Ultralight init failed — ensure DLLs are on the path");
    ul.mount(overlay.path(), html, ViewTrust::LocalAuthored)
        .expect("mount failed");
    pump(&ul, 20);

    let px = sample_center(&ul).expect("center pixel");
    // BGRA: B=0, G=0, R=255, A=255 for solid red
    assert!(
        px[3] > 0 && px[2] > 200 && px[1] < 60 && px[0] < 60,
        "center pixel {:?} is not red — PNG failed to load through scoped FS",
        px
    );
}

#[test]
fn parent_escape_request_is_rejected() {
    if !have_ul() {
        skip_msg(module_path!());
        return;
    }
    let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let buf = init_tracing();
    buf.clear();

    let overlay = tmp_overlay("escape");
    let outside = tmp_overlay("escape_outside");
    fs::write(outside.path().join("secret.png"), b"\x89PNG\r\n").unwrap();

    let outside_name = outside
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let html = format!(
        r#"<!doctype html><html><body style="background:#111;">
        <img src="../{outside_name}/secret.png"/>
    </body></html>"#
    );

    let ul = UlRenderer::init(200, 100, None, &resources_dir()).expect("Ultralight init failed");
    ul.mount(overlay.path(), &html, ViewTrust::LocalAuthored)
        .expect("mount");
    pump(&ul, 10);

    let logs = buf.contents();
    assert!(
        logs.contains("ParentEscape") || logs.contains("parent") || logs.contains("Symlink"),
        "expected a path-escape rejection log line; got:\n{}",
        logs
    );
}

#[test]
fn bundle_installed_trust_rejects_http() {
    if !have_ul() {
        skip_msg(module_path!());
        return;
    }
    let _g = GLOBAL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let buf = init_tracing();
    buf.clear();

    let overlay = tmp_overlay("http_reject");
    let html = r#"<!doctype html><html><body style="background:#222;">
        <img src="http://127.0.0.1:9/nope.png"/>
    </body></html>"#;

    let ul = UlRenderer::init(200, 100, None, &resources_dir()).expect("Ultralight init failed");
    ul.mount(overlay.path(), html, ViewTrust::BundleInstalled)
        .expect("mount");
    pump(&ul, 10);

    let logs = buf.contents();
    assert!(
        logs.contains("non-file://")
            || logs.contains("UnsupportedScheme")
            || logs.contains("http://127.0.0.1"),
        "expected a non-file scheme rejection log line; got:\n{}",
        logs
    );
}
