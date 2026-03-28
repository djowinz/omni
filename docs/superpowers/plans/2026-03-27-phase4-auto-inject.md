# Phase 4: Auto-Inject with Process Watcher and Exclude List

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `--watch` mode to `omni-host` that continuously scans for new processes, detects ones using 3D graphics APIs, and automatically injects the overlay DLL — skipping processes on a user-configurable exclude list.

**Architecture:** The host gains a `Scanner` that polls `CreateToolhelp32Snapshot` every 2 seconds, checks each new process for loaded graphics DLLs (`d3d11.dll`, `d3d12.dll`, `vulkan-1.dll`) and a visible window, filters against the exclude list, and calls the existing `inject_dll`. A JSON config file at `%APPDATA%\Omni\config.json` stores the exclude list and settings. The existing `omni-host <PID> <DLL_PATH>` mode is preserved as-is.

**Tech Stack:** Rust, `windows` crate 0.58 (ToolHelp, WindowsAndMessaging), `serde` + `serde_json` for config, `tracing` for logging.

**Testing notes:** Process enumeration, module checking, and config loading are unit-testable. The full watch loop is tested manually by launching a game while the watcher is running.

**Depends on:** Phase 1 complete (injector works).

---

## File Map

```
host/
  Cargo.toml                         # Add serde, serde_json, new windows features
  src/
    main.rs                          # CLI: parse --watch flag, dispatch to inject or watch mode
    injector/
      mod.rs                         # Unchanged — inject_dll(pid, dll_path)
    config.rs                        # Config struct, load/save, default exclude list
    scanner.rs                       # Process enumeration, DLL detection, window check, poll loop
```

---

### Task 1: Add Dependencies and Windows Features

**Files:**
- Modify: `host/Cargo.toml`

- [ ] **Step 1: Update host/Cargo.toml**

```toml
[package]
name = "omni-host"
version = "0.1.0"
edition = "2021"

[dependencies]
omni-shared = { path = "../shared" }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_System_Threading",
    "Win32_System_Memory",
    "Win32_System_Diagnostics_Debug",
    "Win32_System_Diagnostics_ToolHelp",
    "Win32_System_LibraryLoader",
    "Win32_Security",
    "Win32_UI_WindowsAndMessaging",
]
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p omni-host`
Expected: Downloads serde + serde_json, compiles.

- [ ] **Step 3: Commit**

```bash
git add host/Cargo.toml Cargo.lock
git commit -m "feat(host): add serde, serde_json, and ToolHelp/WindowsAndMessaging windows features"
```

---

### Task 2: Config File — Load, Save, Default Exclude List

**Files:**
- Create: `host/src/config.rs`
- Modify: `host/src/main.rs` (add `mod config;`)

- [ ] **Step 1: Create host/src/config.rs**

```rust
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Application configuration, loaded from %APPDATA%\Omni\config.json.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    /// Process names to never inject into (lowercase, no path).
    pub exclude: Vec<String>,
    /// Poll interval in milliseconds for watch mode.
    pub poll_interval_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            exclude: default_exclude_list(),
            poll_interval_ms: 2000,
        }
    }
}

/// Processes that commonly load D3D11/D3D12 but are not games.
fn default_exclude_list() -> Vec<String> {
    [
        // System
        "dwm.exe",
        "explorer.exe",
        "searchhost.exe",
        "shellexperiencehost.exe",
        "startmenuexperiencehost.exe",
        "systemsettings.exe",
        "textinputhost.exe",
        "widgets.exe",
        // Browsers
        "chrome.exe",
        "firefox.exe",
        "msedge.exe",
        "opera.exe",
        "brave.exe",
        "vivaldi.exe",
        // Communication
        "discord.exe",
        "slack.exe",
        "teams.exe",
        "spotify.exe",
        // Development
        "devenv.exe",
        "code.exe",
        "rider64.exe",
        // Creative / 3D tools
        "blender.exe",
        "photoshop.exe",
        "afterfx.exe",
        "premiere.exe",
        "resolve.exe",
        // Hardware monitoring / overlays (avoid hooking each other)
        "msiafterburner.exe",
        "rtss.exe",
        "hwinfo64.exe",
        "hwinfo32.exe",
        // Game launchers (not the games themselves)
        "steam.exe",
        "steamwebhelper.exe",
        "epicgameslauncher.exe",
        "galaxyclient.exe",
        "gogalaxy.exe",
        "origin.exe",
        "eadesktop.exe",
        "upc.exe",
        "battlenet.exe",
        // Omni itself
        "omni-host.exe",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Return the path to the config file: %APPDATA%\Omni\config.json
pub fn config_path() -> PathBuf {
    std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("Omni"))
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("config.json")
}

/// Load config from disk. Returns default config if file doesn't exist or is invalid.
pub fn load_config(path: &Path) -> Config {
    match std::fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(config) => {
                info!(?path, "Loaded config");
                config
            }
            Err(e) => {
                warn!(?path, error = %e, "Failed to parse config, using defaults");
                Config::default()
            }
        },
        Err(_) => {
            info!(?path, "No config file found, creating with defaults");
            let config = Config::default();
            save_config(path, &config);
            config
        }
    }
}

/// Save config to disk. Creates parent directories if needed.
pub fn save_config(path: &Path, config: &Config) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(config) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                warn!(?path, error = %e, "Failed to save config");
            }
        }
        Err(e) => warn!(error = %e, "Failed to serialize config"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_exclude_list() {
        let config = Config::default();
        assert!(!config.exclude.is_empty());
        assert!(config.exclude.contains(&"chrome.exe".to_string()));
        assert!(config.exclude.contains(&"dwm.exe".to_string()));
        assert!(config.exclude.contains(&"steam.exe".to_string()));
    }

    #[test]
    fn default_poll_interval_is_2_seconds() {
        let config = Config::default();
        assert_eq!(config.poll_interval_ms, 2000);
    }

    #[test]
    fn config_round_trips_through_json() {
        let config = Config {
            exclude: vec!["foo.exe".to_string(), "bar.exe".to_string()],
            poll_interval_ms: 5000,
        };
        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.exclude, config.exclude);
        assert_eq!(loaded.poll_interval_ms, config.poll_interval_ms);
    }

    #[test]
    fn config_deserializes_with_missing_fields() {
        let json = r#"{ "exclude": ["test.exe"] }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.exclude, vec!["test.exe".to_string()]);
        assert_eq!(config.poll_interval_ms, 2000); // default
    }

    #[test]
    fn load_config_returns_default_for_missing_file() {
        let config = load_config(Path::new("/nonexistent/path/config.json"));
        assert!(!config.exclude.is_empty());
    }
}
```

- [ ] **Step 2: Add mod declaration to main.rs**

Add `mod config;` after `mod injector;` in `host/src/main.rs`:

```rust
mod injector;
mod config;
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p omni-host`
Expected: 5 tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/config.rs host/src/main.rs
git commit -m "feat(host): add config module with default exclude list and JSON persistence"
```

---

### Task 3: Scanner — Process Enumeration, DLL Detection, Window Check

**Files:**
- Create: `host/src/scanner.rs`
- Modify: `host/src/main.rs` (add `mod scanner;`)

- [ ] **Step 1: Create host/src/scanner.rs**

```rust
use std::collections::HashSet;

use tracing::{debug, info, warn, trace};
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
use crate::injector;

/// Graphics API DLLs that indicate a process is using 3D rendering.
const GRAPHICS_DLLS: &[&str] = &["d3d11.dll", "d3d12.dll", "vulkan-1.dll"];

pub struct Scanner {
    /// PIDs we've already processed (injected or skipped permanently).
    injected: HashSet<u32>,
    /// Absolute path to the overlay DLL.
    dll_path: String,
    /// Loaded configuration.
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

    /// Run one poll cycle: enumerate processes, filter, inject new ones.
    pub fn poll(&mut self) {
        let processes = match enumerate_processes() {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "Failed to enumerate processes");
                return;
            }
        };

        // Build set of currently-running PIDs for cleanup
        let running: HashSet<u32> = processes.iter().map(|p| p.th32ProcessID).collect();

        // Remove PIDs that are no longer running
        self.injected.retain(|pid| running.contains(pid));

        for entry in &processes {
            let pid = entry.th32ProcessID;

            // Skip if already handled
            if self.injected.contains(&pid) {
                continue;
            }

            // Skip PID 0 (System Idle) and PID 4 (System)
            if pid <= 4 {
                continue;
            }

            let exe_name = wchar_to_string(&entry.szExeFile).to_lowercase();

            // Skip if on exclude list
            if self.config.exclude.iter().any(|e| e == &exe_name) {
                trace!(pid, exe_name, "Skipping excluded process");
                self.injected.insert(pid); // Don't check again
                continue;
            }

            // Check if process has a visible window
            if !has_visible_window(pid) {
                continue; // May gain a window later, don't mark as injected
            }

            // Check if process has loaded a graphics API DLL
            match has_graphics_dll(pid) {
                Ok(true) => {}
                Ok(false) => continue, // May load later, don't mark as injected
                Err(_) => {
                    // Access denied or process exited — skip permanently
                    self.injected.insert(pid);
                    continue;
                }
            }

            // Eligible! Inject.
            info!(pid, exe_name, "Detected graphics process — injecting overlay");

            match injector::inject_dll(pid, &self.dll_path) {
                Ok(()) => {
                    info!(pid, exe_name, "Injection successful");
                }
                Err(e) => {
                    warn!(pid, exe_name, error = %e, "Injection failed");
                }
            }

            // Mark as handled regardless of success (avoid retry loops)
            self.injected.insert(pid);
        }
    }

    /// Number of processes currently tracked as injected.
    pub fn injected_count(&self) -> usize {
        self.injected.len()
    }
}

/// Enumerate all running processes via CreateToolhelp32Snapshot.
fn enumerate_processes() -> windows::core::Result<Vec<PROCESSENTRY32W>> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)? };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    let mut processes = Vec::new();

    unsafe {
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                processes.push(entry);
                entry = PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                match Process32NextW(snapshot, &mut entry) {
                    Ok(()) => {}
                    Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
                    Err(_) => break,
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }

    Ok(processes)
}

/// Check if a process has loaded any graphics API DLL.
fn has_graphics_dll(pid: u32) -> windows::core::Result<bool> {
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid)?
    };

    let mut entry = MODULEENTRY32W {
        dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
        ..Default::default()
    };

    let mut found = false;

    unsafe {
        if Module32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let name = wchar_to_string(&entry.szModule).to_lowercase();

                if GRAPHICS_DLLS.iter().any(|&dll| name == dll) {
                    found = true;
                    break;
                }

                entry = MODULEENTRY32W {
                    dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
                    ..Default::default()
                };
                match Module32NextW(snapshot, &mut entry) {
                    Ok(()) => {}
                    Err(e) if e.code() == ERROR_NO_MORE_FILES.to_hresult() => break,
                    Err(_) => break,
                }
            }
        }
        let _ = CloseHandle(snapshot);
    }

    Ok(found)
}

/// Check if a process has at least one visible window.
fn has_visible_window(target_pid: u32) -> bool {
    struct State {
        pid: u32,
        found: bool,
    }

    let mut state = State {
        pid: target_pid,
        found: false,
    };

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = &mut *(lparam.0 as *mut State);
        let mut window_pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut window_pid));
        if window_pid == state.pid && IsWindowVisible(hwnd).as_bool() {
            state.found = true;
            return BOOL(0); // stop enumeration
        }
        BOOL(1) // continue
    }

    unsafe {
        let _ = EnumWindows(
            Some(callback),
            LPARAM(&mut state as *mut State as isize),
        );
    }

    state.found
}

/// Convert a null-terminated UTF-16 buffer to a Rust String.
fn wchar_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wchar_to_string_converts_null_terminated() {
        let buf = [b'h' as u16, b'i' as u16, 0, 0, 0];
        assert_eq!(wchar_to_string(&buf), "hi");
    }

    #[test]
    fn wchar_to_string_handles_full_buffer() {
        let buf = [b'a' as u16, b'b' as u16, b'c' as u16];
        assert_eq!(wchar_to_string(&buf), "abc");
    }

    #[test]
    fn enumerate_processes_returns_results() {
        // This test runs on the actual system — it should find at least a few processes
        let procs = enumerate_processes().expect("enumerate_processes failed");
        assert!(!procs.is_empty(), "Should find at least one process");
    }

    #[test]
    fn scanner_new_starts_empty() {
        let config = Config::default();
        let scanner = Scanner::new("test.dll".to_string(), config);
        assert_eq!(scanner.injected_count(), 0);
    }
}
```

- [ ] **Step 2: Add mod declaration to main.rs**

Add `mod scanner;` after `mod config;` in `host/src/main.rs`:

```rust
mod injector;
mod config;
mod scanner;
```

- [ ] **Step 3: Verify it compiles and tests pass**

Run: `cargo test -p omni-host`
Expected: All tests pass (5 config + 4 scanner = 9 total).

- [ ] **Step 4: Commit**

```bash
git add host/src/scanner.rs host/src/main.rs
git commit -m "feat(host): add process scanner with graphics DLL detection and visible window check"
```

---

### Task 4: CLI — Add --watch Mode to main.rs

**Files:**
- Modify: `host/src/main.rs`

Update main.rs to support two modes:
- `omni-host <PID> <DLL_PATH>` — inject once (existing behavior)
- `omni-host --watch <DLL_PATH>` — continuously scan and auto-inject

- [ ] **Step 1: Replace host/src/main.rs**

```rust
use std::path::Path;
use std::time::Duration;

use tracing::{info, error};
use tracing_subscriber::EnvFilter;

mod injector;
mod config;
mod scanner;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    if args[1] == "--watch" {
        // Watch mode: omni-host --watch <DLL_PATH>
        if args.len() < 3 {
            eprintln!("Usage: omni-host --watch <DLL_PATH>");
            std::process::exit(1);
        }
        let dll_path = &args[2];
        validate_dll_path(dll_path);
        run_watch_mode(dll_path);
    } else {
        // Single injection: omni-host <PID> <DLL_PATH>
        if args.len() < 3 {
            print_usage();
            std::process::exit(1);
        }
        let pid: u32 = args[1].parse().unwrap_or_else(|_| {
            error!("Invalid PID: {}", args[1]);
            std::process::exit(1);
        });
        let dll_path = &args[2];
        validate_dll_path(dll_path);
        run_inject_once(pid, dll_path);
    }
}

fn print_usage() {
    eprintln!("Usage:");
    eprintln!("  omni-host <PID> <DLL_PATH>      Inject once into a specific process");
    eprintln!("  omni-host --watch <DLL_PATH>     Watch for new games and auto-inject");
}

fn validate_dll_path(dll_path: &str) {
    if !Path::new(dll_path).exists() {
        error!(dll_path, "DLL file not found");
        std::process::exit(1);
    }
}

fn run_inject_once(pid: u32, dll_path: &str) {
    info!(pid, dll_path, "Omni host starting — injecting overlay DLL");

    match injector::inject_dll(pid, dll_path) {
        Ok(()) => info!(pid, "DLL injection successful"),
        Err(e) => {
            error!(pid, error = %e, "DLL injection failed");
            std::process::exit(1);
        }
    }
}

fn run_watch_mode(dll_path: &str) {
    let config_path = config::config_path();
    let config = config::load_config(&config_path);

    let poll_interval = Duration::from_millis(config.poll_interval_ms);

    info!(
        dll_path,
        config_path = ?config_path,
        poll_ms = config.poll_interval_ms,
        exclude_count = config.exclude.len(),
        "Omni host starting in watch mode"
    );
    info!("Press Ctrl+C to stop");

    let mut scanner = scanner::Scanner::new(dll_path.to_string(), config);

    loop {
        scanner.poll();
        std::thread::sleep(poll_interval);
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p omni-host`
Expected: Compiles with no errors.

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host`
Expected: All 9 tests pass.

- [ ] **Step 4: Verify --watch prints usage**

Run: `cargo run -p omni-host -- --watch 2>&1 | head -3`
Expected: Shows usage message about needing DLL_PATH.

- [ ] **Step 5: Commit**

```bash
git add host/src/main.rs
git commit -m "feat(host): add --watch mode for auto-injection with exclude list support"
```

---

### Task 5: Integration Test — Watch Mode with a Real Game

This is a manual integration test.

- [ ] **Step 1: Build everything**

```powershell
cargo build -p omni-host
cargo build -p omni-overlay-dll --release
```

- [ ] **Step 2: Start watch mode**

```powershell
cargo run -p omni-host -- --watch "C:\Users\DyllenOwens\Projects\omni\target\release\omni_overlay_dll.dll"
```

Expected output:
```
INFO omni_host: Omni host starting in watch mode ...
INFO omni_host: Press Ctrl+C to stop
```

- [ ] **Step 3: Launch a DX11 game**

Launch any DX11 game. Within ~2-4 seconds, the watch mode should detect it and inject:
```
INFO omni_host::scanner: Detected graphics process — injecting overlay pid=XXXX exe_name="game.exe"
INFO omni_host::scanner: Injection successful pid=XXXX exe_name="game.exe"
```

Verify the green rectangle overlay appears in-game.

- [ ] **Step 4: Verify exclude list works**

Check that Chrome, Discord, etc. are NOT injected (they should be on the default exclude list). The log should show no injection attempts for excluded processes.

- [ ] **Step 5: Check the config file was created**

```powershell
Get-Content $env:APPDATA\Omni\config.json | Select-Object -First 20
```

Expected: JSON file with `exclude` array and `poll_interval_ms`.

- [ ] **Step 6: Test adding a custom exclude entry**

Edit `%APPDATA%\Omni\config.json` and add a game to the exclude list. Restart watch mode and verify that game is skipped.

- [ ] **Step 7: Troubleshooting**

If auto-injection doesn't trigger:
- Set `RUST_LOG=debug` to see all process scanning output: `$env:RUST_LOG="debug"; cargo run -p omni-host -- --watch ...`
- Set `RUST_LOG=trace` to see excluded processes being skipped
- Check that the game actually loads `d3d11.dll` (the overlay log's graphics modules diagnostic confirms this)
- Games that launch then spawn a separate renderer process may need the child process PID — the scanner will catch it on the next poll

- [ ] **Step 8: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during watch mode integration test"
```

---

## Phase 4 Complete — Summary

At this point you have:

1. `omni-host --watch <DLL_PATH>` — auto-detects and injects into new game processes
2. Default exclude list covering ~40 common non-game processes (browsers, tools, launchers)
3. User-configurable `%APPDATA%\Omni\config.json` for custom excludes and poll interval
4. Existing `omni-host <PID> <DLL_PATH>` still works for manual injection
5. Process scanner cleans up dead PIDs automatically
6. Failed injections are tracked to avoid retry loops

**Next:** Phase 5 will add shared memory IPC between host and overlay DLL, allowing the host to push real sensor data that the overlay renders.
