/// Process scanner: enumerates running processes and injects the overlay DLL
/// into any new process that loads a graphics API (D3D11, D3D12, Vulkan) and
/// has at least one visible window.

use std::collections::HashSet;
use std::mem::size_of;
use std::path::Path;

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
    seen: HashSet<u32>,
    /// PIDs where the DLL was actually successfully injected.
    injected: HashSet<u32>,
    /// PIDs that were already running on the first poll (pre-existing).
    pre_existing: HashSet<u32>,
    /// Absolute path to the overlay DLL on disk.
    dll_path: String,
    /// Filename of the overlay DLL (derived from dll_path).
    dll_filename: String,
    /// Application configuration (exclude list, poll interval, etc.).
    config: Config,
    /// Whether the first poll has run.
    first_poll_done: bool,
    /// Exe name of the most recently injected (or reconnected) process.
    last_injected_exe: Option<String>,
}

impl Scanner {
    pub fn new(dll_path: String, config: Config) -> Self {
        let dll_filename = Path::new(&dll_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("omni_overlay.dll")
            .to_string();

        Self {
            seen: HashSet::new(),
            injected: HashSet::new(),
            pre_existing: HashSet::new(),
            dll_path,
            dll_filename,
            config,
            first_poll_done: false,
            last_injected_exe: None,
        }
    }

    /// Returns the exe name of the most recently injected (or reconnected) process,
    /// or `None` if no injection has occurred yet this session.
    pub fn last_injected_exe(&self) -> Option<&str> {
        self.last_injected_exe.as_deref()
    }

    /// Run one poll cycle: enumerate processes, clean up dead PIDs, and
    /// inject into any eligible process.
    ///
    /// Two-tier injection strategy:
    /// - **New processes** (appeared after host started): Injected if they have
    ///   a visible window, a graphics DLL, and aren't excluded.
    /// - **Pre-existing processes** (already running on first poll): Only injected
    ///   if they also match a known game installation directory, are in the
    ///   include list, or already have the overlay DLL loaded.
    pub fn poll(&mut self) {
        let processes = match enumerate_processes() {
            Ok(p) => p,
            Err(e) => {
                error!(error = %e, "Failed to enumerate processes");
                return;
            }
        };

        // Build set of currently-alive PIDs so we can prune our sets.
        let alive: HashSet<u32> = processes.iter().map(|e| e.th32ProcessID).collect();

        // Remove PIDs that are no longer running.
        self.seen.retain(|pid| alive.contains(pid));
        self.injected.retain(|pid| alive.contains(pid));
        self.pre_existing.retain(|pid| alive.contains(pid));

        // First poll: record all currently-running PIDs as pre-existing.
        if !self.first_poll_done {
            for &pid in &alive {
                self.pre_existing.insert(pid);
            }
            self.first_poll_done = true;
            info!(count = alive.len(), "First poll — recorded pre-existing processes");
            // Don't return — fall through to evaluate pre-existing processes
            // against the game-directory heuristic.
        }

        for entry in &processes {
            let pid = entry.th32ProcessID;

            if pid <= 4 {
                continue;
            }

            if self.seen.contains(&pid) {
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
                self.seen.insert(pid);
                continue;
            }

            // Skip processes running from system directories.
            if is_system_process(pid) {
                debug!(pid, exe_name, "Skipping system directory process");
                self.seen.insert(pid);
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
                    continue;
                }
            };

            if !graphics {
                continue;
            }

            // Check if our DLL is already loaded (e.g. from a previous host session).
            match has_module(pid, &self.dll_filename) {
                Ok(true) => {
                    info!(pid, exe_name, "Overlay DLL already loaded — reconnecting");
                    self.injected.insert(pid);
                    self.last_injected_exe = Some(exe_name.clone());
                    self.seen.insert(pid);
                    continue;
                }
                Ok(false) => {}
                Err(e) => {
                    debug!(pid, exe_name, error = %e, "Could not check for overlay DLL");
                    continue;
                }
            }

            // For pre-existing processes, apply stricter filtering:
            // only inject if the process is in a known game directory or
            // explicitly in the include list.
            if self.pre_existing.contains(&pid) {
                let in_include_list = self
                    .config
                    .include
                    .iter()
                    .any(|inc| inc.eq_ignore_ascii_case(&exe_name));

                if !in_include_list {
                    let exe_path = get_process_exe_path(pid).unwrap_or_default().to_lowercase();
                    let in_game_dir = self
                        .config
                        .game_directories
                        .iter()
                        .any(|dir| exe_path.contains(&dir.to_lowercase()));

                    if !in_game_dir {
                        debug!(pid, exe_name, "Pre-existing process not in game directory — skipping");
                        self.seen.insert(pid);
                        continue;
                    }
                }
            }

            info!(pid, exe_name, "Injecting overlay DLL into process");

            match crate::injector::inject_dll(pid, &self.dll_path) {
                Ok(()) => {
                    info!(pid, exe_name, "Injection successful");
                    self.injected.insert(pid);
                    self.last_injected_exe = Some(exe_name.clone());
                }
                Err(e) => warn!(pid, exe_name, error = %e, "Injection failed"),
            }

            self.seen.insert(pid);
        }
    }

    /// Eject the overlay DLL from all processes that were successfully injected.
    pub fn eject_all(&mut self) {
        let pids: Vec<u32> = self.injected.iter().copied().collect();
        for pid in pids {
            info!(pid, dll_filename = %self.dll_filename, "Ejecting overlay DLL");
            match crate::injector::eject_dll(pid, &self.dll_filename) {
                Ok(()) => {
                    info!(pid, "Ejection successful");
                    self.injected.remove(&pid);
                }
                Err(e) => warn!(pid, error = %e, "Ejection failed"),
            }
        }
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

/// Returns `true` if the given process has a module with the specified name loaded.
pub fn has_module(pid: u32, dll_name: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)?
    };

    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    if unsafe { Module32FirstW(snapshot, &mut entry) }.is_err() {
        unsafe { let _ = CloseHandle(snapshot); }
        return Err("Failed to enumerate modules".into());
    }

    loop {
        let name = wchar_to_string(&entry.szModule);
        if name.eq_ignore_ascii_case(dll_name) {
            unsafe { let _ = CloseHandle(snapshot); }
            return Ok(true);
        }

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

/// Returns the full executable path for a process, or None if inaccessible.
pub fn get_process_exe_path(pid: u32) -> Option<String> {
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid).ok()?
    };

    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    // The first module is always the exe itself.
    let path = if unsafe { Module32FirstW(snapshot, &mut entry) }.is_ok() {
        Some(wchar_to_string(&entry.szExePath))
    } else {
        None
    };

    unsafe { let _ = CloseHandle(snapshot); }
    path
}

/// Returns `true` if the process executable lives in a Windows system directory
/// (e.g. `C:\Windows\`). These are never games.
pub fn is_system_process(pid: u32) -> bool {
    let snapshot = match unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)
    } {
        Ok(s) => s,
        Err(_) => return false, // Can't check — don't assume system
    };

    let mut entry = MODULEENTRY32W {
        dwSize: size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    // The first module is always the exe itself.
    let result = if unsafe { Module32FirstW(snapshot, &mut entry) }.is_ok() {
        let exe_path = wchar_to_string(&entry.szExePath).to_lowercase();
        exe_path.starts_with(r"c:\windows\")
            || exe_path.starts_with(r"c:\program files\windowsapps\")
    } else {
        false
    };

    unsafe { let _ = CloseHandle(snapshot); }
    result
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
        assert!(scanner.injected.is_empty());
        assert!(scanner.seen.is_empty());
        assert!(!scanner.first_poll_done);
    }

    #[test]
    fn last_injected_exe_starts_none() {
        let scanner = Scanner::new("dummy.dll".to_string(), Config::default());
        assert_eq!(scanner.last_injected_exe(), None);
    }
}
