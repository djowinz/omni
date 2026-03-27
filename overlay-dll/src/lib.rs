use std::ffi::c_void;
use windows::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

mod logging;
mod hook;
mod present;
mod shaders;
mod state_backup;
mod renderer;

use logging::log_to_file;

/// DLL entry point. Called by Windows when the DLL is loaded/unloaded.
///
/// # Safety
/// This is called by the Windows loader. We spawn a thread for initialization
/// because the loader lock prevents complex operations in DllMain.
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        x if x == DLL_PROCESS_ATTACH => {
            log_to_file("omni overlay DLL attached — spawning init thread");
            std::thread::spawn(|| {
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
