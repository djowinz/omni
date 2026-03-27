// Hooked Present/ResizeBuffers — Task 4.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::core::HRESULT;

/// Signature for IDXGISwapChain::Present
pub type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT;

/// Signature for IDXGISwapChain::ResizeBuffers
pub type ResizeBuffersFn = unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, u32) -> HRESULT;

/// Original Present function pointer, set by hook.rs during hook installation.
pub static mut ORIGINAL_PRESENT: Option<PresentFn> = None;

/// Original ResizeBuffers function pointer, set by hook.rs during hook installation.
pub static mut ORIGINAL_RESIZE_BUFFERS: Option<ResizeBuffersFn> = None;

/// Total number of frames presented since hook installation.
pub static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);

/// Detour for IDXGISwapChain::Present.
///
/// # Safety
/// Called by the GPU driver — must match the "system" calling convention exactly.
pub unsafe extern "system" fn hooked_present(
    swap_chain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed) + 1;

    if count % 300 == 0 {
        crate::logging::log_to_file(&format!(
            "[present] frame {count}, sync_interval={sync_interval}, flags={flags:#010x}"
        ));
    }

    if let Some(original) = ORIGINAL_PRESENT {
        original(swap_chain, sync_interval, flags)
    } else {
        HRESULT(0) // S_OK fallback
    }
}

/// Detour for IDXGISwapChain::ResizeBuffers.
///
/// # Safety
/// Called by the GPU driver — must match the "system" calling convention exactly.
pub unsafe extern "system" fn hooked_resize_buffers(
    swap_chain: *mut c_void,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: u32,
    swap_chain_flags: u32,
) -> HRESULT {
    crate::logging::log_to_file(&format!(
        "[resize_buffers] buffer_count={buffer_count}, width={width}, height={height}, \
         format={new_format}, flags={swap_chain_flags:#010x}"
    ));

    if let Some(original) = ORIGINAL_RESIZE_BUFFERS {
        original(swap_chain, buffer_count, width, height, new_format, swap_chain_flags)
    } else {
        HRESULT(0) // S_OK fallback
    }
}
