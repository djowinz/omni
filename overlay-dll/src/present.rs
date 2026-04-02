use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use windows::core::HRESULT;

use crate::ipc::SharedMemoryReader;
use crate::logging::log_to_file;
use crate::renderer::OverlayRenderer;

pub type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT;
pub type Present1Fn = unsafe extern "system" fn(*mut c_void, u32, u32, *const c_void) -> HRESULT;
pub type ResizeBuffersFn =
    unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, u32) -> HRESULT;

/// Wrapper for state that is only accessed from a single thread.
/// Implements `Sync` so it can live in a `static`, but the caller
/// must ensure single-threaded access.
#[repr(transparent)]
pub struct SingleThread<T>(pub UnsafeCell<T>);
// SAFETY: All access sites document why they are single-threaded.
unsafe impl<T> Sync for SingleThread<T> {}

pub struct RenderState {
    pub renderer: Option<OverlayRenderer>,
    pub shm_reader: Option<SharedMemoryReader>,
    pub frame_stats: Option<crate::frame_stats::FrameStats>,
    pub original_present: Option<PresentFn>,
    pub original_present1: Option<Present1Fn>,
    pub original_resize_buffers: Option<ResizeBuffersFn>,
}

// SAFETY (RenderState): All fields are accessed exclusively from the render thread
// (the thread that calls Present). ensure_renderer and ensure_shm_reader are
// only called from render_overlay, which only runs from hooked Present/Present1.
// RENDERER_INIT_DONE (AtomicBool) gates one-time init. destroy_renderer is
// called from omni_shutdown after hooks are disabled and drained (200ms sleep).
// The original_* function pointers are written once during install_hooks
// (single init thread) and read from hook callbacks on the render thread.
pub static RENDER_STATE: SingleThread<RenderState> = SingleThread(UnsafeCell::new(RenderState {
    renderer: None,
    shm_reader: None,
    frame_stats: None,
    original_present: None,
    original_present1: None,
    original_resize_buffers: None,
}));

static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);
pub static RENDERER_INIT_DONE: AtomicBool = AtomicBool::new(false);

/// Initialize the renderer on the first Present call.
unsafe fn ensure_renderer(state: &mut RenderState) {
    if RENDERER_INIT_DONE.load(Ordering::Acquire) {
        return;
    }

    match OverlayRenderer::init() {
        Ok(r) => {
            state.renderer = Some(r);
            RENDERER_INIT_DONE.store(true, Ordering::Release);
            state.frame_stats = Some(crate::frame_stats::FrameStats::new());
            log_to_file("[present] frame stats initialized");
            log_to_file("[present] D2D renderer initialized on first frame");
        }
        Err(e) => {
            log_to_file(&format!("[present] FATAL: renderer init failed: {e}"));
            RENDERER_INIT_DONE.store(true, Ordering::Release);
        }
    }
}

/// Try to open shared memory if not already open.
unsafe fn ensure_shm_reader(state: &mut RenderState) {
    if state.shm_reader.is_some() {
        return;
    }
    if let Some(reader) = SharedMemoryReader::open() {
        state.shm_reader = Some(reader);
    }
    // If it fails, we'll try again next frame — host might not be running yet
}

/// Common rendering logic shared by hooked_present and hooked_present1.
unsafe fn render_overlay(state: &mut RenderState, swap_chain: *mut c_void) {
    ensure_renderer(state);
    ensure_shm_reader(state);

    // Record frame timing
    if let Some(frame_stats) = &mut state.frame_stats {
        frame_stats.record();

        // Write frame stats + render dimensions to shared memory
        if frame_stats.available() {
            if let Some(reader) = &state.shm_reader {
                // Get swap chain dimensions for the host's layout viewport
                let (rw, rh) = state.renderer
                    .as_ref()
                    .map(|r| r.get_render_size(swap_chain))
                    .unwrap_or((0, 0));

                let fd = omni_shared::FrameData {
                    fps: frame_stats.fps(),
                    frame_time_ms: frame_stats.frame_time_ms(),
                    frame_time_avg_ms: frame_stats.frame_time_avg_ms(),
                    frame_time_1percent_ms: frame_stats.frame_time_1pct_ms(),
                    frame_time_01percent_ms: frame_stats.frame_time_01pct_ms(),
                    available: true,
                    render_width: rw,
                    render_height: rh,
                };
                reader.write_frame_data(&fd);
            }
        }
    }

    let renderer = match &mut state.renderer {
        Some(r) => r,
        None => return,
    };

    // Read widgets from shared memory
    let widgets = match &mut state.shm_reader {
        Some(reader) => {
            let slot = reader.read_current();
            let count = slot.widget_count as usize;
            if count > 0 {
                &slot.widgets[..count]
            } else {
                return;
            }
        }
        None => return,
    };

    // Copy widgets to a local buffer so we can override FPS text
    let mut local_widgets: Vec<omni_shared::ComputedWidget> = widgets.to_vec();

    // Override frame timing values in widget text.
    // The host stores the raw template in label_text (e.g., "{fps}: AVG")
    // and the interpolated text in format_pattern (e.g., "N/A: AVG").
    // The DLL replaces {fps}/{frame-time}/etc placeholders in the template
    // with its own computed values, preserving the user's formatting.
    if let Some(frame_stats) = &state.frame_stats {
        if frame_stats.available() {
            for widget in &mut local_widgets {
                let template = omni_shared::read_fixed_str(&widget.label_text);
                if template.is_empty() {
                    continue;
                }

                // Check if this template contains any frame timing placeholders
                let has_fps = template.contains("{fps}");
                let has_ft = template.contains("{frame-time}");
                let has_ft_avg = template.contains("{frame-time.avg}");
                let has_ft_1pct = template.contains("{frame-time.1pct}");
                let has_ft_01pct = template.contains("{frame-time.01pct}");

                if has_fps || has_ft || has_ft_avg || has_ft_1pct || has_ft_01pct {
                    let mut text = template.to_string();
                    if has_fps {
                        text = text.replace("{fps}", &format!("{:.0}", frame_stats.fps()));
                    }
                    if has_ft {
                        text = text.replace(
                            "{frame-time}",
                            &format!("{:.1}", frame_stats.frame_time_ms()),
                        );
                    }
                    if has_ft_avg {
                        text = text.replace(
                            "{frame-time.avg}",
                            &format!("{:.1}", frame_stats.frame_time_avg_ms()),
                        );
                    }
                    if has_ft_1pct {
                        text = text.replace(
                            "{frame-time.1pct}",
                            &format!("{:.1}", frame_stats.frame_time_1pct_ms()),
                        );
                    }
                    if has_ft_01pct {
                        text = text.replace(
                            "{frame-time.01pct}",
                            &format!("{:.1}", frame_stats.frame_time_01pct_ms()),
                        );
                    }
                    omni_shared::write_fixed_str(&mut widget.format_pattern, &text);
                }
            }
        }
    }

    renderer.render(swap_chain, &local_widgets);
}

/// Drop the renderer and shared memory reader. Called during shutdown.
pub unsafe fn destroy_renderer() {
    // SAFETY: Called from omni_shutdown after hooks are disabled and drained (200ms sleep).
    // No render thread can be accessing RENDER_STATE at this point.
    let state = &mut *RENDER_STATE.0.get();
    RENDERER_INIT_DONE.store(false, Ordering::SeqCst);
    if let Some(renderer) = state.renderer.take() {
        drop(renderer);
        log_to_file("[present] D2D renderer destroyed");
    }
    if let Some(reader) = state.shm_reader.take() {
        drop(reader);
        log_to_file("[present] shared memory reader closed");
    }
    state.frame_stats = None;
    log_to_file("[present] frame stats destroyed");
}

/// # Safety
/// Called by the DXGI runtime via minhook trampoline. `swap_chain` is the
/// same pointer the game passed to the original Present function.
pub unsafe extern "system" fn hooked_present(
    swap_chain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    // SAFETY: hooked_present is only called from the render thread (DXGI Present callback).
    // No other thread accesses RENDER_STATE concurrently — hooks are disabled before
    // destroy_renderer runs, with a 200ms drain window.
    let state = &mut *RENDER_STATE.0.get();

    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
    if count.is_multiple_of(300) {
        log_to_file(&format!(
            "[present] frame {count}, sync_interval={sync_interval}, flags={flags:#010x}"
        ));
    }

    render_overlay(state, swap_chain);

    if let Some(original) = state.original_present {
        original(swap_chain, sync_interval, flags)
    } else {
        HRESULT(0)
    }
}

/// # Safety
/// Called by the DXGI runtime via minhook trampoline. `swap_chain` is the
/// same pointer the game passed to the original Present function.
pub unsafe extern "system" fn hooked_present1(
    swap_chain: *mut c_void,
    sync_interval: u32,
    present_flags: u32,
    present_params: *const c_void,
) -> HRESULT {
    // SAFETY: Same as hooked_present — single render thread access.
    let state = &mut *RENDER_STATE.0.get();

    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
    if count.is_multiple_of(300) {
        log_to_file(&format!(
            "[present1] frame {count}, sync_interval={sync_interval}, flags={present_flags:#010x}"
        ));
    }

    render_overlay(state, swap_chain);

    if let Some(original) = state.original_present1 {
        original(swap_chain, sync_interval, present_flags, present_params)
    } else {
        HRESULT(0)
    }
}

/// # Safety
/// Called by the DXGI runtime via minhook trampoline. `swap_chain` is the
/// same pointer the game passed to the original Present function.
pub unsafe extern "system" fn hooked_resize_buffers(
    swap_chain: *mut c_void,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: u32,
    swap_chain_flags: u32,
) -> HRESULT {
    // SAFETY: Same as hooked_present — single render thread access.
    let state = &mut *RENDER_STATE.0.get();

    log_to_file(&format!(
        "[resize_buffers] {width}x{height}, buffers={buffer_count}"
    ));

    // Release D2D render target before resize
    if let Some(renderer) = &mut state.renderer {
        renderer.release_render_target();
    }

    // Call original ResizeBuffers
    let result = if let Some(original) = state.original_resize_buffers {
        original(
            swap_chain,
            buffer_count,
            width,
            height,
            new_format,
            swap_chain_flags,
        )
    } else {
        HRESULT(0)
    };

    // Recreate render target after resize
    if result.is_ok() {
        if let Some(renderer) = &mut state.renderer {
            if let Err(e) = renderer.recreate_render_target(swap_chain) {
                log_to_file(&format!(
                    "[resize_buffers] failed to recreate render target: {e}"
                ));
            }
        }
    }

    result
}
