use std::ffi::c_void;
use windows::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

mod logging;
mod hook;
mod present;

use logging::log_to_file;

#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        x if x == DLL_PROCESS_ATTACH => {
            log_to_file("omni overlay DLL attached to process");
        }
        x if x == DLL_PROCESS_DETACH => {
            log_to_file("omni overlay DLL detached from process");
        }
        _ => {}
    }
    TRUE
}
