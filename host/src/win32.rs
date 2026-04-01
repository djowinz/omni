//! Thin Win32 helpers: RAII handle wrapper and Toolhelp32 iterators.
//!
//! These helpers eliminate duplicated Toolhelp32 snapshot iteration
//! across `scanner.rs` and `injector/mod.rs`.

use std::mem::size_of;

use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW, Process32NextW,
    MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
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

/// Compare a null-terminated UTF-16 buffer to an ASCII string, case-insensitive.
/// Returns true if they match. Does not allocate.
pub fn wchar_eq_ignore_ascii_case(buf: &[u16], ascii: &str) -> bool {
    let mut buf_iter = buf.iter().copied();
    let mut str_iter = ascii.bytes();

    loop {
        match (buf_iter.next(), str_iter.next()) {
            (Some(0), None) | (None, None) => return true,
            (Some(w), Some(a)) => {
                if w > 127 || !a.is_ascii() {
                    return false;
                }
                if !(w as u8).eq_ignore_ascii_case(&a) {
                    return false;
                }
            }
            _ => return false,
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
    let snapshot = OwnedHandle::new(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? });

    let mut entries = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: `entry.dwSize` is set to the correct struct size.
    // `snapshot` is a valid Toolhelp32 handle.
    unsafe { Process32FirstW(snapshot.raw(), &mut entry) }?;

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
    Ok(modules
        .iter()
        .any(|m| wchar_eq_ignore_ascii_case(&m.szModule, dll_name)))
}

/// Find a module's base address and file path in one snapshot (case-insensitive).
pub fn find_remote_module(
    pid: u32,
    dll_name: &str,
) -> Result<Option<(*const std::ffi::c_void, String)>, HostError> {
    let modules = iter_modules(pid)?;
    Ok(modules
        .iter()
        .find(|m| wchar_eq_ignore_ascii_case(&m.szModule, dll_name))
        .map(|m| {
            (
                m.modBaseAddr as *const std::ffi::c_void,
                wchar_to_string(&m.szExePath),
            )
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wchar_eq_case_insensitive() {
        let buf: Vec<u16> = "d3d11.dll\0".encode_utf16().collect();
        assert!(wchar_eq_ignore_ascii_case(&buf, "D3D11.DLL"));
        assert!(wchar_eq_ignore_ascii_case(&buf, "d3d11.dll"));
        assert!(!wchar_eq_ignore_ascii_case(&buf, "d3d12.dll"));
    }

    #[test]
    fn wchar_eq_empty() {
        let buf: Vec<u16> = vec![0];
        assert!(wchar_eq_ignore_ascii_case(&buf, ""));
        assert!(!wchar_eq_ignore_ascii_case(&buf, "x"));
    }

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
        assert!(
            !processes.is_empty(),
            "Expected at least one running process"
        );
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
