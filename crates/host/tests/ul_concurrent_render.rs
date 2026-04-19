//! Regression test for the fs_dispatcher mount-ID routing fix.
//!
//! Reproduces the pattern that caused `STATUS_STACK_BUFFER_OVERRUN` before
//! the `fs_dispatcher::ACTIVE`-slot-to-`MOUNTS`-routing refactor: two
//! concurrent `UlRenderer` instances, each rendering its own overlay, with
//! interleaved `update_and_render` ticks. Under the old design the second
//! mount overwrote the global active slot and the first renderer's
//! callbacks dereferenced a swapped-out FS.
//!
//! `#[ignore]`-gated: requires Ultralight's platform resources dir to be
//! adjacent to the test binary. Run with:
//!   cargo test --ignored -p host --test ul_concurrent_render

use std::path::Path;

use omni_host::omni::view_trust::ViewTrust;
use omni_host::ul_renderer::UlRenderer;
use tempfile::TempDir;

/// Build a resources dir next to the test binary. Mirrors the pattern
/// `thumbnail_integration.rs` uses.
fn resources_dir_from_test_exe() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

fn trivial_html() -> &'static str {
    "<!doctype html><html><head><meta charset=\"utf-8\"></head>\
     <body style=\"background:#000;color:#fff\">concurrent render test</body></html>"
}

fn mount_and_render(renderer: &UlRenderer, overlay_root: &Path) {
    renderer
        .mount(overlay_root, trivial_html(), ViewTrust::ThumbnailGen)
        .expect("mount");
    for _ in 0..3 {
        renderer.update_and_render();
    }
}

/// Two `UlRenderer` instances active at the same time, each rooted at its
/// own tempdir. Under the new routing-table dispatcher each owns its
/// `MountHandle`, so the second mount does not disturb the first.
///
/// Assertion: neither renderer panics, and `with_pixels` on each returns a
/// non-empty BGRA buffer after rendering. If the dispatcher regressed to a
/// single-slot design, this would crash with `STATUS_STACK_BUFFER_OVERRUN`
/// (Windows) or a segfault (other platforms) rather than fail cleanly.
#[test]
#[ignore = "requires Ultralight resources; run with --ignored after placing resources in target/debug/deps/"]
fn two_ultralight_renderers_concurrent_no_crash() {
    let resources = resources_dir_from_test_exe();
    let tmp_a = TempDir::new().expect("tempdir a");
    let tmp_b = TempDir::new().expect("tempdir b");

    let a = UlRenderer::init(800, 450, &resources).expect("renderer a");
    let b = UlRenderer::init(800, 450, &resources).expect("renderer b");

    // Interleave: mount A, mount B, tick A, tick B, tick A, tick B.
    a.mount(tmp_a.path(), trivial_html(), ViewTrust::ThumbnailGen)
        .expect("mount a");
    b.mount(tmp_b.path(), trivial_html(), ViewTrust::ThumbnailGen)
        .expect("mount b");
    for _ in 0..3 {
        a.update_and_render();
        b.update_and_render();
    }

    let mut a_has_pixels = false;
    a.with_pixels(|w, h, _row, px, _dirty| {
        a_has_pixels = w > 0 && h > 0 && !px.is_empty();
    });
    let mut b_has_pixels = false;
    b.with_pixels(|w, h, _row, px, _dirty| {
        b_has_pixels = w > 0 && h > 0 && !px.is_empty();
    });

    assert!(a_has_pixels, "renderer A did not produce pixels");
    assert!(b_has_pixels, "renderer B did not produce pixels");
}

/// Regression for the specific case the backlog #2 firefight hit:
/// a first renderer is already ticking continuously when a second
/// renderer is constructed, mounted, and torn down. The first renderer's
/// callbacks must not dereference the second renderer's overlay_root
/// during or after the second renderer's lifetime.
#[test]
#[ignore = "requires Ultralight resources; run with --ignored after placing resources in target/debug/deps/"]
fn first_renderer_survives_second_renderers_full_lifecycle() {
    let resources = resources_dir_from_test_exe();
    let tmp_a = TempDir::new().expect("tempdir a");

    let a = UlRenderer::init(800, 450, &resources).expect("renderer a");
    mount_and_render(&a, tmp_a.path());

    // Scope the second renderer so its Drop fires mid-test.
    {
        let tmp_b = TempDir::new().expect("tempdir b");
        let b = UlRenderer::init(800, 450, &resources).expect("renderer b");
        mount_and_render(&b, tmp_b.path());
        // Drop of `b` here removes mount_b from the dispatcher map;
        // tempdir_b cleanup happens when `tmp_b` drops just after.
    }

    // Renderer A continues ticking. If the dispatcher is keyed correctly,
    // A's callbacks still resolve against its own mount_a entry. If the
    // old single-slot design were in place, the dispatcher would now
    // have `None` (cleared by B's drop) and A would fail to resolve.
    for _ in 0..3 {
        a.update_and_render();
    }

    let mut a_still_rendering = false;
    a.with_pixels(|w, h, _row, px, _dirty| {
        a_still_rendering = w > 0 && h > 0 && !px.is_empty();
    });
    assert!(
        a_still_rendering,
        "renderer A stopped producing pixels after renderer B's lifecycle"
    );
}
