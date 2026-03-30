use std::ffi::c_void;
use windows::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

mod logging;
mod hook;
mod present;
mod renderer;
mod ipc;

use logging::log_to_file;

/// Module handle for this DLL, saved on attach for use during shutdown.
static mut DLL_MODULE: Option<HINSTANCE> = None;

/// DLL entry point. Called by Windows when the DLL is loaded/unloaded.
///
/// # Safety
/// This is called by the Windows loader. We spawn a thread for initialization
/// because the loader lock prevents complex operations in DllMain.
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        x if x == DLL_PROCESS_ATTACH => {
            DLL_MODULE = Some(hinst);
            log_to_file("omni overlay DLL attached — spawning init thread");
            std::thread::spawn(|| {
                // Wait for the game's graphics stack to initialize before creating
                // dummy devices for vtable discovery. Without this delay, creating
                // a D3D11 device races with the game's D3D12/DXGI initialization
                // and can crash (especially on game restart when injection is fast).
                std::thread::sleep(std::time::Duration::from_secs(10));
                log_to_file("[init] starting hook installation after startup delay");

                if let Err(e) = unsafe { hook::install_hooks() } {
                    log_to_file(&format!("FATAL: hook installation failed: {e}"));
                }
            });
        }
        x if x == DLL_PROCESS_DETACH => {
            log_to_file("omni overlay DLL detached from process");
        }
        _ => {}
    }
    TRUE
}

/// Exported shutdown function. The host calls this via CreateRemoteThread.
///
/// This runs on its own thread (not under loader lock), so it can safely:
/// 1. Disable all minhook trampolines (restores original vtable pointers)
/// 2. Sleep to let any in-flight hook calls on the render thread complete
/// 3. Call FreeLibraryAndExitThread to atomically unload and exit
///
/// # Safety
/// Must be called via CreateRemoteThread with the parameter ignored.
#[no_mangle]
pub unsafe extern "system" fn omni_shutdown(_param: *mut c_void) -> u32 {
    log_to_file("[shutdown] disabling all hooks");

    if let Err(e) = minhook::MinHook::disable_all_hooks() {
        log_to_file(&format!("[shutdown] WARNING: disable_all_hooks failed: {e:?}"));
    }

    // Give the render thread time to finish any in-flight hook call.
    // After this, no more hook calls will enter our code since hooks are disabled.
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Release all D3D resources (shaders, buffers, RTV, device/context refs).
    // Must happen AFTER hooks are disabled and drained so no render call races.
    present::destroy_renderer();

    // Clear the initialization guard so a fresh injection can re-initialize.
    hook::HOOKS_INSTALLED.store(false, std::sync::atomic::Ordering::SeqCst);

    log_to_file("[shutdown] hooks disabled, resources released, unloading DLL");

    // Get our own module handle to pass to FreeLibraryAndExitThread.
    if let Ok(hmod) = GetModuleHandleA(windows::core::s!("omni_overlay_dll.dll")) {
        windows::Win32::System::LibraryLoader::FreeLibraryAndExitThread(hmod, 0);
    }

    // FreeLibraryAndExitThread never returns, but just in case:
    0
}
