/// Process scanner: enumerates running processes and spawns the external
/// overlay for any new process with at least one visible window.
use std::collections::{HashMap, HashSet};
use std::process::Child;

use tracing::{error, info, warn};
use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible,
};

use crate::config::Config;
use crate::win32;

pub struct Scanner {
    seen: HashSet<u32>,
    tracked: HashMap<u32, Child>,
    overlay_exe_path: String,
    config: Config,
    last_game_exe: Option<String>,
    last_external_pid: Option<u32>,
    last_game_hwnd: Option<isize>,
    last_process_count: usize,
    last_seen_count: usize,
}

impl Scanner {
    pub fn new(
        overlay_exe_path: String,
        config: Config,
    ) -> Self {
        Self {
            seen: HashSet::new(),
            tracked: HashMap::new(),
            overlay_exe_path,
            config,
            last_process_count: 0,
            last_seen_count: 0,
            last_game_exe: None,
            last_external_pid: None,
            last_game_hwnd: None,
        }
    }

    pub fn last_game_exe(&self) -> Option<&str> {
        self.last_game_exe.as_deref()
    }

    pub fn last_external_pid(&self) -> Option<u32> {
        self.last_external_pid
    }

    pub fn last_game_hwnd(&self) -> Option<isize> {
        self.last_game_hwnd
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
        let process_count = alive.len();

        self.seen.retain(|pid| alive.contains(pid));
        let seen_count = self.seen.len();

        if process_count != self.last_process_count || seen_count != self.last_seen_count {
            info!(process_count, seen = seen_count, tracked = self.tracked.len(), "Scanner poll");
            self.last_process_count = process_count;
            self.last_seen_count = seen_count;
        }
        let prev_tracked_count = self.tracked.len();
        self.tracked.retain(|pid, overlay_child| {
            if alive.contains(pid) {
                true
            } else {
                if let Err(e) = overlay_child.kill() {
                    warn!(error = %e, "Failed to kill external overlay process");
                }
                false
            }
        });
        // If all tracked games exited, clear stale game state
        if self.tracked.is_empty() && prev_tracked_count > 0 {
            self.last_game_exe = None;
            self.last_game_hwnd = None;
            self.last_external_pid = None;
        }

        for entry in &processes {
            let pid = entry.th32ProcessID;

            if pid <= 4 {
                continue;
            }
            if self.seen.contains(&pid) {
                continue;
            }

            let exe_name = win32::wchar_to_string(&entry.szExeFile);

            // Always skip our own processes and the Electron app
            const SELF_EXECUTABLES: &[&str] = &[
                "omni-host.exe",
                "omni-overlay.exe",
                "omni.exe",       // Installed Electron app
                "electron.exe",   // Dev mode Electron
                "nextron.exe",    // Dev mode Nextron
            ];
            if SELF_EXECUTABLES.iter().any(|s| s.eq_ignore_ascii_case(&exe_name)) {
                self.seen.insert(pid);
                continue;
            }

            // Explicit include list — always spawn overlay, skip all other checks
            let in_include = self
                .config
                .include
                .iter()
                .any(|inc| inc.eq_ignore_ascii_case(&exe_name));
            if in_include {
                if let Some(game_hwnd) = find_visible_window(pid) {
                    info!(pid, exe_name, "Spawning overlay (include list)");
                    self.last_game_hwnd = Some(game_hwnd.0 as isize);
                    self.spawn_external_overlay(pid, &exe_name, game_hwnd);
                    self.seen.insert(pid); // Only mark seen after successful spawn
                } else {
                    info!(pid, exe_name, "Pending overlay — no visible window yet (include list)");
                }
                continue;
            }

            let excluded = self
                .config
                .exclude
                .iter()
                .any(|ex| ex.eq_ignore_ascii_case(&exe_name));
            if excluded {
                self.seen.insert(pid);
                continue;
            }

            // Skip common helper/launcher patterns that live inside game directories
            // but aren't games themselves
            let exe_lower = exe_name.to_lowercase();
            let is_helper = exe_lower.contains("helper")
                || exe_lower.contains("launcher")
                || exe_lower.contains("crashhandler")
                || exe_lower.contains("crashreport")
                || exe_lower.contains("crashpad")
                || exe_lower.contains("webhelper")
                || exe_lower.contains("prereq")
                || exe_lower.contains("installer")
                || exe_lower.contains("setup")
                || exe_lower.contains("updater")
                || exe_lower.contains("unins");
            if is_helper {
                self.seen.insert(pid);
                continue;
            }

            // Check if this process lives in a known game directory
            let exe_path = match win32::get_process_exe_path(pid) {
                Ok(p) => p,
                Err(_) => {
                    // Can't query path — retry next poll (process may become
                    // queryable later, or anti-cheat may relax after startup).
                    // Do NOT add to seen — we want to retry.
                    continue;
                }
            };
            let exe_path_lower = exe_path.to_lowercase();
            let in_game_dir = self
                .config
                .game_directories
                .iter()
                .any(|dir| exe_path_lower.contains(&dir.to_lowercase()));
            if !in_game_dir {
                // Definitively not a game — add to seen, never retry
                self.seen.insert(pid);
                continue;
            }

            // Require a visible window — retry if not yet visible
            let game_hwnd = match find_visible_window(pid) {
                Some(h) => h,
                None => {
                    info!(pid, exe_name, "Pending overlay — no visible window yet (game directory)");
                    continue;
                }
            };

            info!(pid, exe_name, "Spawning overlay");
            self.last_game_hwnd = Some(game_hwnd.0 as isize);
            self.spawn_external_overlay(pid, &exe_name, game_hwnd);
            self.seen.insert(pid);
        }
    }

    /// Kill all tracked external overlay child processes.
    pub fn kill_all(&mut self) {
        for (pid, mut child) in self.tracked.drain() {
            info!(pid, "Killing external overlay process");
            if let Err(e) = child.kill() {
                warn!(error = %e, "Failed to kill external overlay process");
            }
        }
    }

    /// Iterate over tracked game PIDs.
    pub fn tracked_pids(&self) -> impl Iterator<Item = &u32> {
        self.tracked.keys()
    }

    /// Check if a PID is currently tracked.
    pub fn is_tracked(&self, pid: u32) -> bool {
        self.tracked.contains_key(&pid)
    }

    fn spawn_external_overlay(&mut self, pid: u32, exe_name: &str, hwnd: HWND) {
        let hwnd_value = hwnd.0 as isize;
        info!(pid, exe_name, hwnd = hwnd_value, "Spawning external overlay process");

        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        match std::process::Command::new(&self.overlay_exe_path)
            .args(["--hwnd", &hwnd_value.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
        {
            Ok(child) => {
                self.tracked.insert(pid, child);
                self.last_game_exe = Some(exe_name.to_string());
                self.last_external_pid = Some(pid);
            }
            Err(e) => {
                warn!(pid, exe_name, error = %e, "Failed to spawn external overlay process");
            }
        }
    }
}

// Scanner-specific helpers (not shared)

fn find_visible_window(pid: u32) -> Option<HWND> {
    struct CallbackData {
        target_pid: u32,
        found_hwnd: Option<HWND>,
    }

    // SAFETY: This callback is invoked synchronously by EnumWindows below.
    // `lparam` points to a valid `CallbackData` on the stack, which outlives
    // the EnumWindows call.
    unsafe extern "system" fn enum_cb(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut CallbackData);
        let mut window_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
        if window_pid == data.target_pid && IsWindowVisible(hwnd).as_bool() {
            data.found_hwnd = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut data = CallbackData {
        target_pid: pid,
        found_hwnd: None,
    };
    let _ = unsafe { EnumWindows(Some(enum_cb), LPARAM(&mut data as *mut _ as isize)) };
    data.found_hwnd
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn scanner_new_starts_empty() {
        let scanner = Scanner::new(
            "overlay.exe".to_string(),
            Config::default(),
        );
        assert!(scanner.tracked.is_empty());
        assert!(scanner.seen.is_empty());
    }

    #[test]
    fn last_game_exe_starts_none() {
        let scanner = Scanner::new(
            "overlay.exe".to_string(),
            Config::default(),
        );
        assert_eq!(scanner.last_game_exe(), None);
    }
}
