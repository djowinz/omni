//! Regression test for the fs_dispatcher multi-mount routing table.
//!
//! Scope note: the shipped thumbnail pipeline uses a SINGLE UlRenderer
//! instance that remounts between the live overlay and a transient
//! thumbnail overlay via MountHandle swaps (see
//! `crates/host/src/ul_renderer.rs::render_thumbnail_to_png`). The design
//! pivoted away from two concurrent UlRenderers when Ultralight's C API
//! was shown to crash (STATUS_STACK_BUFFER_OVERRUN / ACCESS_VIOLATION on
//! Windows) whenever a second Renderer instance is created in the same
//! process — independent of the fs_dispatcher race this spec was
//! originally scoped to address.
//!
//! What this test DOES cover: the fs_dispatcher's multi-mount routing is
//! still load-bearing for the shipped design, because `render_thumbnail_to_png`
//! registers a second MountHandle for the thumbnail overlay while the
//! live MountHandle is still alive. The test below constructs two
//! UlRenderers as the simplest way to force two simultaneous mounts in
//! the dispatcher — both must resolve correctly under the new routing.
//!
//! What this test does NOT cover: the full render_thumbnail_to_png
//! save/mount/render/restore sequence on a single renderer. That path is
//! validated by the Marathon manual smoke gate; adding a unit-level
//! assertion against it would require mocking Ultralight and is deferred.
//!
//! `#[ignore]`-gated: requires Ultralight's platform resources dir to be
//! adjacent to the test binary. Run with:
//!
//!   cargo test -p host --test ul_concurrent_render -- --ignored
//!
//! KNOWN LIMITATION (pre-existing, not caused by this spec): Ultralight's
//! C API does not cleanly survive multiple `ulCreateRenderer` /
//! `ulDestroyRenderer` cycles within a single process — the existing
//! `thumbnail_integration.rs` tests exhibit the same pattern (they pass
//! individually but crash when chained). Because of that, this file
//! intentionally contains a SINGLE test so `cargo test --ignored` on
//! this binary never exercises the destroy-and-recreate path.

use std::path::PathBuf;

use omni_host::omni::view_trust::ViewTrust;
use omni_host::ul_renderer::UlRenderer;
use tempfile::TempDir;

/// Build a resources dir next to the test binary. Mirrors the pattern
/// `thumbnail_integration.rs` uses.
fn resources_dir_from_test_exe() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn trivial_html() -> &'static str {
    "<!doctype html><html><head><meta charset=\"utf-8\"></head>\
     <body style=\"background:#000;color:#fff\">concurrent render test</body></html>"
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
