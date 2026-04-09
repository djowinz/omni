//! Thin Win32 helpers: RAII handle wrapper and Toolhelp32 iterators.
//!
//! These helpers eliminate duplicated Toolhelp32 snapshot iteration
//! across `scanner.rs`.

use std::mem::size_of;

use windows::Win32::Foundation::{CloseHandle, ERROR_NO_MORE_FILES, HANDLE, MAX_PATH};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
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

/// Get the full executable path for a process by PID.
///
/// Uses `QueryFullProcessImageNameW` which only requires `PROCESS_QUERY_LIMITED_INFORMATION`,
/// so it works even on processes protected by anti-cheat.
pub fn get_process_exe_path(pid: u32) -> Result<String, HostError> {
    // SAFETY: OpenProcess returns a valid handle on success (checked via `?`).
    let handle =
        OwnedHandle::new(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid)? });

    let mut buf = [0u16; MAX_PATH as usize];
    let mut len = buf.len() as u32;

    // SAFETY: `handle` is valid, `buf` is large enough, `len` is set correctly.
    unsafe {
        QueryFullProcessImageNameW(
            handle.raw(),
            PROCESS_NAME_FORMAT(0),
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )?;
    }

    Ok(String::from_utf16_lossy(&buf[..len as usize]))
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
        assert!(
            !processes.is_empty(),
            "Expected at least one running process"
        );
    }
}
