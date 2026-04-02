use std::ffi::c_void;
use windows::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows::Win32::Graphics::Gdi::{AddFontResourceExW, FONT_RESOURCE_CHARACTERISTICS};
use windows::Win32::System::LibraryLoader::GetModuleHandleA;
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

mod frame_stats;
mod hook;
mod ipc;
mod logging;
mod present;
mod renderer;

use logging::log_to_file;

/// Module handle for this DLL, saved on attach for use during shutdown.
///
/// # Safety
/// Written once in `DllMain` (DLL_PROCESS_ATTACH), read once in `omni_shutdown`.
/// Both are serialized by the Windows loader / our own call sequence.
static DLL_MODULE: crate::present::SingleThread<Option<HINSTANCE>> =
    crate::present::SingleThread(std::cell::UnsafeCell::new(None));

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
            *DLL_MODULE.0.get() = Some(hinst);
            log_to_file("omni overlay DLL attached — spawning init thread");

            // Register bundled feather icon font for DirectWrite
            register_bundled_fonts(hinst);

            std::thread::spawn(|| {
                // SAFETY: install_hooks is called on a dedicated thread (not
                // under loader lock). It accesses static mut globals that
                // are not yet initialized, so no data race.
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
        log_to_file(&format!(
            "[shutdown] WARNING: disable_all_hooks failed: {e:?}"
        ));
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

    // SAFETY: GetModuleHandleA with a known module name returns a valid
    // HMODULE. FreeLibraryAndExitThread atomically unloads and exits.
    if let Ok(hmod) = GetModuleHandleA(windows::core::s!("omni_overlay.dll")) {
        windows::Win32::System::LibraryLoader::FreeLibraryAndExitThread(hmod, 0);
    }

    // FreeLibraryAndExitThread never returns, but just in case:
    0
}

/// Register bundled font files so DirectWrite can use them.
/// Looks for .ttf/.otf files in the same directory as the DLL.
unsafe fn register_bundled_fonts(hinst: HINSTANCE) {
    // Get the DLL's directory path
    let mut path_buf = [0u16; 260];
    let len = windows::Win32::System::LibraryLoader::GetModuleFileNameW(hinst, &mut path_buf);
    if len == 0 {
        return;
    }

    let dll_path = String::from_utf16_lossy(&path_buf[..len as usize]);
    let dll_dir = match dll_path.rfind('\\') {
        Some(pos) => &dll_path[..pos],
        None => return,
    };

    // Register feather.ttf if it exists next to the DLL
    let font_path = format!("{}\\feather.ttf", dll_dir);
    let font_wide: Vec<u16> = font_path.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: AddFontResourceExW registers the font for this process only
    // (FR_PRIVATE = 0x10). No admin rights needed, no system-wide changes.
    let result = AddFontResourceExW(
        windows::core::PCWSTR(font_wide.as_ptr()),
        FONT_RESOURCE_CHARACTERISTICS(0x10), // FR_PRIVATE
        None,
    );

    if result > 0 {
        log_to_file(&format!("[fonts] registered feather.ttf ({} fonts added)", result));
    } else {
        log_to_file(&format!("[fonts] failed to register feather.ttf from {}", font_path));
    }
}
