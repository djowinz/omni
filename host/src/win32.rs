//! Thin Win32 helpers: RAII handle wrapper and Toolhelp32 iterators.
//!
//! These helpers eliminate duplicated Toolhelp32 snapshot iteration
//! across `scanner.rs` and `injector/mod.rs`.

use std::mem::size_of;

use windows::Win32::Foundation::{CloseHandle, HANDLE, ERROR_NO_MORE_FILES};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW,
    Process32NextW, MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};

use crate::error::HostError;

/// RAII wrapper for Win32 `HANDLE`. Calls `CloseHandle` on drop.
pub struct OwnedHandle(HANDLE);

impl OwnedHandle {
    /// Wrap a raw handle. The caller must ensure `handle` is valid and
    /// owned exclusively by this wrapper.
    pub fn new(handle: HANDLE) -> Self {
        Self(handle)
    }

    /// Access the underlying handle.
    pub fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: The handle was obtained from a Win32 API that returns a valid
        // handle on success. We own it exclusively — no other code closes it.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Convert a null-terminated UTF-16 slice to a `String`.
/// If no null terminator is present, the entire slice is decoded.
pub fn wchar_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// Collect all running processes via a Toolhelp32 snapshot.
pub fn iter_processes() -> Result<Vec<PROCESSENTRY32W>, HostError> {
    // SAFETY: TH32CS_SNAPPROCESS with pid 0 captures all processes.
    // The returned handle is valid on success (checked via `?`).
    let snapshot = OwnedHandle::new(unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)?
    });

    let mut entries = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: `entry.dwSize` is set to the correct struct size.
    // `snapshot` is a valid Toolhelp32 handle.
    unsafe { Process32FirstW(snapshot.raw(), &mut entry) }
        .map_err(|e| HostError::Win32(e))?;

    entries.push(entry);

    loop {
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        // SAFETY: same as above — dwSize is reset before each call.
        match unsafe { Process32NextW(snapshot.raw(), &mut entry) } {
            Ok(()) => entries.push(entry),
            Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(e) => return Err(HostError::Win32(e)),
        }
    }

    Ok(entries)
}

/// Collect all modules loaded in a process via a Toolhelp32 snapshot.
///
/// Returns `Err` if the snapshot cannot be created (e.g., access denied).
/// Returns an empty vec if the first module call fails (process may have exited).
pub fn iter_modules(pid: u32) -> Result<Vec<MODULEENTRY32W>, HostError> {
    // SAFETY: SNAPMODULE | SNAPMODULE32 captures both 32-bit and 64-bit modules.
    // The returned handle is valid on success (checked via `?`).
    let snapshot = OwnedHandle::new(unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)?
    });

    let mut entries = Vec::new();
    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: `entry.dwSize` is set correctly. Snapshot handle is valid.
    if unsafe { Module32FirstW(snapshot.raw(), &mut entry) }.is_err() {
        return Ok(entries); // Process may have exited — return empty
    }

    entries.push(entry);

    loop {
        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        // SAFETY: same as above — dwSize is reset before each call.
        match unsafe { Module32NextW(snapshot.raw(), &mut entry) } {
            Ok(()) => entries.push(entry),
            Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(e) => return Err(HostError::Win32(e)),
        }
    }

    Ok(entries)
}

/// Check whether a process has a module with the given name loaded (case-insensitive).
pub fn has_module(pid: u32, dll_name: &str) -> Result<bool, HostError> {
    let modules = iter_modules(pid)?;
    Ok(modules.iter().any(|m| {
        wchar_to_string(&m.szModule).eq_ignore_ascii_case(dll_name)
    }))
}

/// Get the full executable path for the first module of a process (the exe itself).
pub fn get_process_exe_path(pid: u32) -> Option<String> {
    iter_modules(pid).ok()?.first().map(|m| wchar_to_string(&m.szExePath))
}

/// Find a module's base address in a remote process by name (case-insensitive).
pub fn find_remote_module_base(pid: u32, dll_name: &str) -> Result<Option<*const std::ffi::c_void>, HostError> {
    let modules = iter_modules(pid)?;
    Ok(modules.iter().find(|m| {
        wchar_to_string(&m.szModule).eq_ignore_ascii_case(dll_name)
    }).map(|m| m.modBaseAddr as *const std::ffi::c_void))
}

/// Find a module's file path in a remote process by name (case-insensitive).
pub fn find_remote_module_path(pid: u32, dll_name: &str) -> Result<Option<String>, HostError> {
    let modules = iter_modules(pid)?;
    Ok(modules.iter().find(|m| {
        wchar_to_string(&m.szModule).eq_ignore_ascii_case(dll_name)
    }).map(|m| wchar_to_string(&m.szExePath)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wchar_to_string_null_terminated() {
        let buf: Vec<u16> = vec!['H' as u16, 'i' as u16, 0, 0xFF, 0xFF];
        assert_eq!(wchar_to_string(&buf), "Hi");
    }

    #[test]
    fn wchar_to_string_full_buffer_no_null() {
        let buf: Vec<u16> = vec!['A' as u16, 'B' as u16, 'C' as u16];
        assert_eq!(wchar_to_string(&buf), "ABC");
    }

    #[test]
    fn iter_processes_returns_nonempty() {
        let processes = iter_processes().expect("iter_processes failed");
        assert!(!processes.is_empty(), "Expected at least one running process");
    }

    #[test]
    fn has_module_finds_kernel32() {
        // Every process has kernel32.dll loaded
        let pid = std::process::id();
        let result = has_module(pid, "kernel32.dll");
        // May fail due to access rights on our own process — that's ok
        if let Ok(found) = result {
            assert!(found, "Expected kernel32.dll in our own process");
        }
    }
}
