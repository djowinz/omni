use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows::core::HRESULT;

use crate::logging::log_to_file;
use crate::renderer::OverlayRenderer;

pub type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT;
pub type Present1Fn = unsafe extern "system" fn(*mut c_void, u32, u32, *const c_void) -> HRESULT;
pub type ResizeBuffersFn = unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, u32) -> HRESULT;

pub static mut ORIGINAL_PRESENT: Option<PresentFn> = None;
pub static mut ORIGINAL_PRESENT1: Option<Present1Fn> = None;
pub static mut ORIGINAL_RESIZE_BUFFERS: Option<ResizeBuffersFn> = None;

static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);

/// Global renderer, initialized on first Present call.
static RENDERER_INIT_DONE: AtomicBool = AtomicBool::new(false);
static mut RENDERER: Option<OverlayRenderer> = None;

/// Initialize the renderer on the first Present call.
unsafe fn ensure_renderer(swap_chain: *mut c_void) {
    if RENDERER_INIT_DONE.load(Ordering::Acquire) {
        return;
    }

    match OverlayRenderer::init(swap_chain) {
        Ok(r) => {
            RENDERER = Some(r);
            RENDERER_INIT_DONE.store(true, Ordering::Release);
            log_to_file("[present] renderer initialized on first frame");
        }
        Err(e) => {
            log_to_file(&format!("[present] FATAL: renderer init failed: {e}"));
            // Prevent retry spam — mark as done even on failure.
            RENDERER_INIT_DONE.store(true, Ordering::Release);
        }
    }
}

/// Drop the renderer and release all D3D resources. Called during shutdown
/// before the DLL is unloaded.
pub unsafe fn destroy_renderer() {
    RENDERER_INIT_DONE.store(false, Ordering::SeqCst);
    if let Some(renderer) = RENDERER.take() {
        drop(renderer);
        log_to_file("[present] renderer destroyed — D3D resources released");
    }
}

/// Common rendering logic shared by hooked_present and hooked_present1.
unsafe fn render_overlay(swap_chain: *mut c_void) {
    ensure_renderer(swap_chain);
    if let Some(renderer) = &RENDERER {
        renderer.render(swap_chain);
    }
}

pub unsafe extern "system" fn hooked_present(
    swap_chain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
    if count % 300 == 0 {
        log_to_file(&format!(
            "[present] frame {count}, sync_interval={sync_interval}, flags={flags:#010x}"
        ));
    }

    render_overlay(swap_chain);

    if let Some(original) = ORIGINAL_PRESENT {
        original(swap_chain, sync_interval, flags)
    } else {
        HRESULT(0)
    }
}

pub unsafe extern "system" fn hooked_present1(
    swap_chain: *mut c_void,
    sync_interval: u32,
    present_flags: u32,
    present_params: *const c_void,
) -> HRESULT {
    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
    if count % 300 == 0 {
        log_to_file(&format!(
            "[present1] frame {count}, sync_interval={sync_interval}, flags={present_flags:#010x}"
        ));
    }

    render_overlay(swap_chain);

    if let Some(original) = ORIGINAL_PRESENT1 {
        original(swap_chain, sync_interval, present_flags, present_params)
    } else {
        HRESULT(0)
    }
}

pub unsafe extern "system" fn hooked_resize_buffers(
    swap_chain: *mut c_void,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: u32,
    swap_chain_flags: u32,
) -> HRESULT {
    log_to_file(&format!(
        "[resize_buffers] {width}x{height}, buffers={buffer_count}"
    ));

    // Release RTV before resize (holding a reference blocks resize)
    if let Some(renderer) = &mut RENDERER {
        renderer.release_rtv();
    }

    // Call original ResizeBuffers
    let result = if let Some(original) = ORIGINAL_RESIZE_BUFFERS {
        original(swap_chain, buffer_count, width, height, new_format, swap_chain_flags)
    } else {
        HRESULT(0)
    };

    // Recreate RTV after resize
    if result.is_ok() {
        if let Some(renderer) = &mut RENDERER {
            if let Err(e) = renderer.recreate_rtv(swap_chain) {
                log_to_file(&format!("[resize_buffers] failed to recreate RTV: {e}"));
            }
        }
    }

    result
}
