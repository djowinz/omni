/// Process scanner: enumerates running processes and injects the overlay DLL
/// into any new process that loads a graphics API (D3D11, D3D12, Vulkan) and
/// has at least one visible window.

use std::collections::HashSet;
use std::mem::size_of;

use tracing::{debug, error, info, warn};
use windows::Win32::Foundation::{CloseHandle, BOOL, HWND, LPARAM, ERROR_NO_MORE_FILES};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, Process32FirstW,
    Process32NextW, MODULEENTRY32W, PROCESSENTRY32W, TH32CS_SNAPMODULE,
    TH32CS_SNAPMODULE32, TH32CS_SNAPPROCESS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
};

use crate::config::Config;

/// Graphics DLLs that indicate a process is using a hardware-accelerated
/// rendering API we care about.
const GRAPHICS_DLLS: &[&str] = &["d3d11.dll", "d3d12.dll", "vulkan-1.dll"];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub struct Scanner {
    /// PIDs that have already been processed (injected or skipped).
    injected: HashSet<u32>,
    /// Absolute path to the overlay DLL on disk.
    dll_path: String,
    /// Application configuration (exclude list, poll interval, etc.).
    config: Config,
}

impl Scanner {
    pub fn new(dll_path: String, config: Config) -> Self {
        Self {
            injected: HashSet::new(),
            dll_path,
            config,
        }
    }

    /// Run one poll cycle: enumerate processes, clean up dead PIDs, and
    /// inject into any new eligible process.
    pub fn poll(&mut self) {
        let processes = match enumerate_processes() {
            Ok(p) => p,
            Err(e) => {
                error!(error = %e, "Failed to enumerate processes");
                return;
            }
        };

        // Build set of currently-alive PIDs so we can prune our injected set.
        let alive: HashSet<u32> = processes.iter().map(|e| e.th32ProcessID).collect();

        // Remove PIDs that are no longer running.
        self.injected.retain(|pid| alive.contains(pid));

        for entry in &processes {
            let pid = entry.th32ProcessID;

            // Skip System Idle Process (0), System (4), and other kernel PIDs.
            if pid <= 4 {
                continue;
            }

            // Skip already-handled PIDs.
            if self.injected.contains(&pid) {
                continue;
            }

            let exe_name = wchar_to_string(&entry.szExeFile);

            // Skip excluded process names (case-insensitive).
            let excluded = self
                .config
                .exclude
                .iter()
                .any(|ex| ex.eq_ignore_ascii_case(&exe_name));
            if excluded {
                debug!(pid, exe_name, "Skipping excluded process");
                // Still mark as handled so we don't re-check every cycle.
                self.injected.insert(pid);
                continue;
            }

            // Skip processes with no visible window.
            if !has_visible_window(pid) {
                continue;
            }

            // Skip processes that haven't loaded a graphics DLL.
            let graphics = match has_graphics_dll(pid) {
                Ok(v) => v,
                Err(e) => {
                    debug!(pid, exe_name, error = %e, "Could not check modules (access denied?)");
                    // Do NOT mark as handled — the process might become
                    // accessible later, but to avoid a busy-loop we skip until
                    // the next visible-window check naturally gates us.
                    continue;
                }
            };

            if !graphics {
                continue;
            }

            info!(pid, exe_name, "Injecting overlay DLL into process");

            match crate::injector::inject_dll(pid, &self.dll_path) {
                Ok(()) => info!(pid, exe_name, "Injection successful"),
                Err(e) => warn!(pid, exe_name, error = %e, "Injection failed"),
            }

            // Mark as handled regardless of success to avoid retry loops.
            self.injected.insert(pid);
        }
    }

    /// Number of PIDs currently tracked (injected or skipped).
    pub fn injected_count(&self) -> usize {
        self.injected.len()
    }
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Returns all currently-running processes via a Toolhelp32 snapshot.
pub fn enumerate_processes() -> Result<Vec<PROCESSENTRY32W>, Box<dyn std::error::Error>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? };

    let mut processes = Vec::new();
    let mut entry = PROCESSENTRY32W {
        dwSize: size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    // Retrieve the first entry.
    let ok = unsafe { Process32FirstW(snapshot, &mut entry) };
    if let Err(e) = ok {
        unsafe { let _ = CloseHandle(snapshot); }
        return Err(e.into());
    }

    processes.push(entry);

    loop {
        // dwSize must be reset before each call.
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        match unsafe { Process32NextW(snapshot, &mut entry) } {
            Ok(()) => processes.push(entry),
            Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(e) => {
                unsafe { let _ = CloseHandle(snapshot); }
                return Err(e.into());
            }
        }
    }

    unsafe { let _ = CloseHandle(snapshot); }
    Ok(processes)
}

/// Returns `true` if the given process has loaded at least one of the
/// recognised graphics DLLs (D3D11, D3D12, Vulkan-1).
pub fn has_graphics_dll(pid: u32) -> Result<bool, Box<dyn std::error::Error>> {
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)?
    };

    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    let ok = unsafe { Module32FirstW(snapshot, &mut entry) };
    if let Err(e) = ok {
        unsafe { let _ = CloseHandle(snapshot); }
        return Err(e.into());
    }

    loop {
        let module_name = wchar_to_string(&entry.szModule).to_ascii_lowercase();
        if GRAPHICS_DLLS.iter().any(|&dll| module_name == dll) {
            unsafe { let _ = CloseHandle(snapshot); }
            return Ok(true);
        }

        // dwSize must be reset before each call.
        entry.dwSize = size_of::<MODULEENTRY32W>() as u32;
        match unsafe { Module32NextW(snapshot, &mut entry) } {
            Ok(()) => {}
            Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
            Err(e) => {
                unsafe { let _ = CloseHandle(snapshot); }
                return Err(e.into());
            }
        }
    }

    unsafe { let _ = CloseHandle(snapshot); }
    Ok(false)
}

/// Returns `true` if the given process owns at least one visible top-level window.
pub fn has_visible_window(pid: u32) -> bool {
    // We pass `pid` through LPARAM and collect the result via a bool pointer.
    struct CallbackData {
        target_pid: u32,
        found: bool,
    }

    unsafe extern "system" fn enum_windows_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut CallbackData);
        let mut window_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
        if window_pid == data.target_pid && IsWindowVisible(hwnd).as_bool() {
            data.found = true;
            return BOOL(0); // stop enumeration
        }
        BOOL(1) // continue
    }

    let mut data = CallbackData {
        target_pid: pid,
        found: false,
    };

    let _ = unsafe {
        EnumWindows(
            Some(enum_windows_cb),
            LPARAM(&mut data as *mut _ as isize),
        )
    };

    data.found
}

/// Converts a null-terminated UTF-16 slice to a `String`.
/// If no null terminator is present, the entire slice is decoded.
pub fn wchar_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn wchar_to_string_null_terminated() {
        // "Hi" followed by a null and some garbage.
        let buf: Vec<u16> = vec!['H' as u16, 'i' as u16, 0, 0xFF, 0xFF];
        assert_eq!(wchar_to_string(&buf), "Hi");
    }

    #[test]
    fn wchar_to_string_full_buffer_no_null() {
        // No null terminator — entire buffer should be decoded.
        let buf: Vec<u16> = vec!['A' as u16, 'B' as u16, 'C' as u16];
        assert_eq!(wchar_to_string(&buf), "ABC");
    }

    #[test]
    fn enumerate_processes_returns_nonempty() {
        // This runs against the real system, so there must be at least one
        // process (this test runner itself).
        let processes = enumerate_processes().expect("enumerate_processes failed");
        assert!(!processes.is_empty(), "Expected at least one running process");
    }

    #[test]
    fn scanner_new_starts_empty() {
        let scanner = Scanner::new("dummy.dll".to_string(), Config::default());
        assert_eq!(scanner.injected_count(), 0);
    }
}
