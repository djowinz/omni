/// Process scanner: enumerates running processes and injects the overlay DLL
/// into any new process that loads a graphics API (D3D11, D3D12, Vulkan) and
/// has at least one visible window.

use std::collections::HashSet;

use tracing::{debug, error, info, warn};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
};

use crate::config::Config;
use crate::win32;

/// Graphics DLLs that indicate a process is using a hardware-accelerated
/// rendering API we care about.
const GRAPHICS_DLLS: &[&str] = &["d3d11.dll", "d3d12.dll", "vulkan-1.dll"];

pub struct Scanner {
    seen: HashSet<u32>,
    injected: HashSet<u32>,
    pre_existing: HashSet<u32>,
    dll_path: String,
    dll_filename: String,
    config: Config,
    first_poll_done: bool,
    last_injected_exe: Option<String>,
}

impl Scanner {
    pub fn new(dll_path: String, config: Config) -> Self {
        let dll_filename = std::path::Path::new(&dll_path)
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

    pub fn last_injected_exe(&self) -> Option<&str> {
        self.last_injected_exe.as_deref()
    }

    pub fn poll(&mut self) {
        let processes = match win32::iter_processes() {
            Ok(p) => p,
            Err(e) => {
                error!(error = %e, "Failed to enumerate processes");
                return;
            }
        };

        let alive: HashSet<u32> = processes.iter().map(|e| e.th32ProcessID).collect();

        self.seen.retain(|pid| alive.contains(pid));
        self.injected.retain(|pid| alive.contains(pid));
        self.pre_existing.retain(|pid| alive.contains(pid));

        if !self.first_poll_done {
            for &pid in &alive {
                self.pre_existing.insert(pid);
            }
            self.first_poll_done = true;
            info!(count = alive.len(), "First poll — recorded pre-existing processes");
        }

        for entry in &processes {
            let pid = entry.th32ProcessID;

            if pid <= 4 { continue; }
            if self.seen.contains(&pid) { continue; }

            let exe_name = win32::wchar_to_string(&entry.szExeFile);

            let excluded = self.config.exclude.iter()
                .any(|ex| ex.eq_ignore_ascii_case(&exe_name));
            if excluded {
                debug!(pid, exe_name, "Skipping excluded process");
                self.seen.insert(pid);
                continue;
            }

            if is_system_process(pid) {
                debug!(pid, exe_name, "Skipping system directory process");
                self.seen.insert(pid);
                continue;
            }

            if !has_visible_window(pid) { continue; }

            let graphics = match has_graphics_dll(pid) {
                Ok(v) => v,
                Err(e) => {
                    debug!(pid, exe_name, error = %e, "Could not check modules (access denied?)");
                    continue;
                }
            };
            if !graphics { continue; }

            match win32::has_module(pid, &self.dll_filename) {
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

            if self.pre_existing.contains(&pid) {
                let in_include_list = self.config.include.iter()
                    .any(|inc| inc.eq_ignore_ascii_case(&exe_name));
                if !in_include_list {
                    let exe_path = win32::get_process_exe_path(pid).unwrap_or_default().to_lowercase();
                    let in_game_dir = self.config.game_directories.iter()
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

// Scanner-specific helpers (not shared)

fn has_graphics_dll(pid: u32) -> Result<bool, crate::error::HostError> {
    let modules = win32::iter_modules(pid)?;
    Ok(modules.iter().any(|m| {
        let name = win32::wchar_to_string(&m.szModule).to_ascii_lowercase();
        GRAPHICS_DLLS.iter().any(|&dll| name == dll)
    }))
}

fn is_system_process(pid: u32) -> bool {
    match win32::get_process_exe_path(pid) {
        Some(path) => {
            let lower = path.to_lowercase();
            lower.starts_with(r"c:\windows\")
                || lower.starts_with(r"c:\program files\windowsapps\")
        }
        None => false,
    }
}

fn has_visible_window(pid: u32) -> bool {
    struct CallbackData {
        target_pid: u32,
        found: bool,
    }

    // SAFETY: This callback is invoked synchronously by EnumWindows below.
    // `lparam` points to a valid `CallbackData` on the stack, which outlives
    // the EnumWindows call. The cast is safe because we control both sides.
    unsafe extern "system" fn enum_windows_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut CallbackData);
        let mut window_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
        if window_pid == data.target_pid && IsWindowVisible(hwnd).as_bool() {
            data.found = true;
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut data = CallbackData { target_pid: pid, found: false };

    // SAFETY: `data` lives on the stack and is valid for the duration of
    // EnumWindows. The callback receives a pointer to it via LPARAM.
    let _ = unsafe {
        EnumWindows(Some(enum_windows_cb), LPARAM(&mut data as *mut _ as isize))
    };

    data.found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

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
