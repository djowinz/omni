# Idiomatic Rust Simplification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate code duplication, standardize error handling, modernize static patterns, add RAII guards for Win32 handles and remote memory, and document all unsafe blocks across the omni codebase.

**Architecture:** Three-phase crate-by-crate approach (shared → host → dll). The host phase introduces three small abstractions: `OwnedHandle` (RAII for Win32 handles), `RemoteAlloc` (RAII for remote process memory), and `iter_modules`/`iter_processes` (Toolhelp32 iterators). A `HostError` enum replaces all ad-hoc error types.

**Tech Stack:** Rust 1.80+ (for `std::sync::LazyLock`), `windows` crate v0.58, `minhook` v0.9

**Spec:** `docs/superpowers/specs/2026-03-31-idiomatic-rust-simplification-design.md`

---

## File Structure

### New Files
- `host/src/error.rs` — `HostError` enum + `From` impls + `Display`
- `host/src/win32.rs` — `OwnedHandle`, `wchar_to_string`, `iter_processes`, `iter_modules`

### Modified Files (by task)
- `shared/src/widget_types.rs` — `fill(0)` in `write_fixed_str`
- `shared/src/ipc_protocol.rs` — SAFETY comments on atomics
- `shared/src/sensor_types.rs` — import ordering
- `shared/src/lib.rs` — no changes expected
- `host/src/main.rs` — import ordering, use `HostError`, module declarations for `error` and `win32`
- `host/src/scanner.rs` — rewrite to use `win32::` helpers, `HostError`, SAFETY comments
- `host/src/injector/mod.rs` — rewrite to use `win32::` helpers, `RemoteAlloc`, `HostError`, SAFETY comments
- `host/src/ipc/mod.rs` — `HostError`, SAFETY comments
- `host/src/watcher.rs` — `HostError`
- `host/src/ws_server.rs` — import ordering
- `host/src/sensors/mod.rs` — import ordering
- `host/src/sensors/gpu.rs` — SAFETY comments
- `host/src/sensors/cpu.rs` — import ordering
- `host/src/sensors/cpu_temp.rs` — import ordering
- `host/src/sensors/ram.rs` — import ordering
- `host/src/omni/expression.rs` — `LazyLock` modernization
- `host/src/omni/resolver.rs` — import ordering, SAFETY comments
- `overlay-dll/src/lib.rs` — SAFETY comments
- `overlay-dll/src/hook.rs` — SAFETY comments
- `overlay-dll/src/present.rs` — SAFETY comments
- `overlay-dll/src/renderer.rs` — SAFETY comments, import ordering
- `overlay-dll/src/frame_stats.rs` — SAFETY comments
- `overlay-dll/src/ipc/mod.rs` — SAFETY comments

---

## Phase 1: omni-shared

### Task 1: Clean up `write_fixed_str` and add SAFETY comments to shared crate

**Files:**
- Modify: `shared/src/widget_types.rs:191-201`
- Modify: `shared/src/ipc_protocol.rs:38-53`
- Modify: `shared/src/sensor_types.rs:1-3`

- [ ] **Step 1: Replace byte-by-byte zeroing with `fill(0)` in `write_fixed_str`**

In `shared/src/widget_types.rs`, replace lines 196-200:

```rust
    dest[copy_len] = 0;
    // Zero the rest
    for byte in &mut dest[copy_len + 1..] {
        *byte = 0;
    }
```

With:

```rust
    // Null-terminate and zero remaining bytes
    dest[copy_len..].fill(0);
```

- [ ] **Step 2: Add SAFETY comments to `ipc_protocol.rs` atomic operations**

In `shared/src/ipc_protocol.rs`, add SAFETY comments to the three atomic methods:

Before `reader_slot_index` (line 38):
```rust
    /// Returns the index (0 or 1) of the slot the DLL should read.
    pub fn reader_slot_index(&self) -> usize {
        // SAFETY: Acquire ordering ensures we see all writes the host
        // performed before flipping the slot. The `& 1` mask guarantees
        // the result is always 0 or 1, so it's a valid slot index.
        self.active_slot.load(Ordering::Acquire) as usize & 1
    }
```

Before `writer_slot_index` (line 43):
```rust
    /// Returns the index (0 or 1) of the slot the host should write to.
    pub fn writer_slot_index(&self) -> usize {
        // SAFETY: Same ordering as reader_slot_index. XOR with 1 gives
        // the opposite slot (0→1 or 1→0).
        (self.active_slot.load(Ordering::Acquire) as usize & 1) ^ 1
    }
```

Before `flip_slot` (line 49):
```rust
    /// Host calls this after writing to the writer slot to make it active.
    pub fn flip_slot(&self) {
        // SAFETY: Release ordering ensures all writes to the slot are
        // visible to readers before the slot index changes. Only the host
        // thread calls this, so no concurrent stores race.
        let current = self.active_slot.load(Ordering::Acquire);
        let next = current ^ 1;
        self.active_slot.store(next, Ordering::Release);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-shared`
Expected: All existing tests pass.

- [ ] **Step 4: Commit**

```bash
git add shared/src/widget_types.rs shared/src/ipc_protocol.rs
git commit -m "cleanup(shared): idiomatic fill(0) and SAFETY comments on atomics"
```

---

## Phase 2: omni-host

### Task 2: Add `HostError` type

**Files:**
- Create: `host/src/error.rs`
- Modify: `host/src/main.rs:109-117` (add `mod error;`)

- [ ] **Step 1: Create `host/src/error.rs`**

```rust
//! Unified error type for the omni-host crate.

use std::fmt;

/// Covers the three failure domains in the host: Win32 API errors,
/// standard I/O errors, and freeform messages.
#[derive(Debug)]
pub enum HostError {
    Win32(windows::core::Error),
    Io(std::io::Error),
    Message(String),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Win32(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Message(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for HostError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Win32(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Message(_) => None,
        }
    }
}

impl From<windows::core::Error> for HostError {
    fn from(e: windows::core::Error) -> Self {
        Self::Win32(e)
    }
}

impl From<std::io::Error> for HostError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<String> for HostError {
    fn from(s: String) -> Self {
        Self::Message(s)
    }
}

impl From<&str> for HostError {
    fn from(s: &str) -> Self {
        Self::Message(s.to_string())
    }
}
```

- [ ] **Step 2: Register the module in `main.rs`**

In `host/src/main.rs`, add `mod error;` alongside the other module declarations (after line 109):

```rust
mod error;
mod injector;
mod config;
mod scanner;
mod sensors;
mod ipc;
mod ws_server;
mod omni;
mod workspace;
mod watcher;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p omni-host`
Expected: Compiles with no new errors.

- [ ] **Step 4: Commit**

```bash
git add host/src/error.rs host/src/main.rs
git commit -m "feat(host): add HostError unified error type"
```

---

### Task 3: Add `win32` helpers module (`OwnedHandle`, `wchar_to_string`, iterators)

**Files:**
- Create: `host/src/win32.rs`
- Modify: `host/src/main.rs` (add `mod win32;`)

- [ ] **Step 1: Write tests for `OwnedHandle` and `wchar_to_string`**

Create `host/src/win32.rs` with tests first:

```rust
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
```

- [ ] **Step 2: Register the module in `main.rs`**

In `host/src/main.rs`, add `pub(crate) mod win32;` with the other module declarations:

```rust
mod error;
pub(crate) mod win32;
mod injector;
mod config;
mod scanner;
mod sensors;
mod ipc;
mod ws_server;
mod omni;
mod workspace;
mod watcher;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host win32`
Expected: All 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/win32.rs host/src/main.rs
git commit -m "feat(host): add win32 helpers — OwnedHandle, Toolhelp32 iterators"
```

---

### Task 4: Refactor `scanner.rs` to use `win32::` helpers

**Files:**
- Modify: `host/src/scanner.rs`

- [ ] **Step 1: Replace imports and remove duplicated functions**

Rewrite `host/src/scanner.rs`. The entire helper section (lines 238-443: `enumerate_processes`, `has_module`, `has_graphics_dll`, `get_process_exe_path`, `is_system_process`, `has_visible_window`, `wchar_to_string`) is replaced. The `Scanner` struct and its `poll`/`eject_all` methods stay, but now call `crate::win32::*` instead of local helpers.

```rust
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
        let processes = match win32::iter_processes() {
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

            // For pre-existing processes, apply stricter filtering.
            if self.pre_existing.contains(&pid) {
                let in_include_list = self
                    .config
                    .include
                    .iter()
                    .any(|inc| inc.eq_ignore_ascii_case(&exe_name));

                if !in_include_list {
                    let exe_path = win32::get_process_exe_path(pid).unwrap_or_default().to_lowercase();
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
// Helper functions (scanner-specific, not shared)
// ---------------------------------------------------------------------------

/// Returns `true` if the given process has loaded at least one of the
/// recognised graphics DLLs (D3D11, D3D12, Vulkan-1).
fn has_graphics_dll(pid: u32) -> Result<bool, crate::error::HostError> {
    let modules = win32::iter_modules(pid)?;
    Ok(modules.iter().any(|m| {
        let name = win32::wchar_to_string(&m.szModule).to_ascii_lowercase();
        GRAPHICS_DLLS.iter().any(|&dll| name == dll)
    }))
}

/// Returns `true` if the process executable lives in a Windows system directory.
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

/// Returns `true` if the given process owns at least one visible top-level window.
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
            return BOOL(0); // stop enumeration
        }
        BOOL(1) // continue
    }

    let mut data = CallbackData {
        target_pid: pid,
        found: false,
    };

    // SAFETY: `data` lives on the stack and is valid for the duration of
    // EnumWindows. The callback receives a pointer to it via LPARAM.
    let _ = unsafe {
        EnumWindows(
            Some(enum_windows_cb),
            LPARAM(&mut data as *mut _ as isize),
        )
    };

    data.found
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

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
```

- [ ] **Step 2: Update any external callers of `scanner::wchar_to_string` and `scanner::enumerate_processes`**

In `host/src/main.rs`, replace calls to `scanner::wchar_to_string` and `scanner::enumerate_processes` in `run_stop()` with `win32::wchar_to_string` and `win32::iter_processes`:

Replace `scanner::enumerate_processes()` with `win32::iter_processes()`.
Replace `scanner::wchar_to_string(&entry.szExeFile)` with `win32::wchar_to_string(&entry.szExeFile)`.
Replace `scanner::has_module(pid, dll_name)` with `win32::has_module(pid, dll_name)`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass. The old `wchar_to_string` tests now live in `win32::tests`.

- [ ] **Step 4: Commit**

```bash
git add host/src/scanner.rs host/src/main.rs
git commit -m "refactor(host): scanner uses win32:: helpers, eliminates duplicated Toolhelp32 code"
```

---

### Task 5: Refactor `injector/mod.rs` to use `win32::` helpers + `RemoteAlloc` RAII

**Files:**
- Modify: `host/src/injector/mod.rs`

- [ ] **Step 1: Rewrite `injector/mod.rs`**

Replace the entire file. Key changes:
- All `find_remote_module`, `find_remote_module_path` functions deleted — replaced by `win32::find_remote_module_base` and `win32::find_remote_module_path`
- `RemoteAlloc` guard replaces manual `VirtualFreeEx` at 3 error paths
- All functions return `Result<(), HostError>` instead of `Box<dyn Error>`
- All `CloseHandle` calls replaced by `OwnedHandle`
- All unsafe blocks get `// SAFETY:` comments

```rust
/// DLL injection via CreateRemoteThread + LoadLibraryW.
///
/// This module opens the target process, allocates memory for the DLL path,
/// writes the path as UTF-16, and creates a remote thread that calls LoadLibraryW.
///
/// Requires the host process to have sufficient privileges (usually admin or
/// same-user) to open the target process with the necessary access rights.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use tracing::{debug, info};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Diagnostics::Debug::WriteProcessMemory;
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, WaitForSingleObject, PROCESS_ALL_ACCESS,
};
use windows::core::{s, w};

use crate::error::HostError;
use crate::win32::{self, OwnedHandle};

/// RAII guard for memory allocated in a remote process via `VirtualAllocEx`.
/// Automatically calls `VirtualFreeEx` on drop.
struct RemoteAlloc {
    process: HANDLE,
    ptr: *mut std::ffi::c_void,
}

impl RemoteAlloc {
    /// Consume the guard without freeing the memory (e.g., after successful use).
    fn leak(self) -> *mut std::ffi::c_void {
        let ptr = self.ptr;
        std::mem::forget(self);
        ptr
    }
}

impl Drop for RemoteAlloc {
    fn drop(&mut self) {
        // SAFETY: `self.process` is a valid handle for the lifetime of the
        // injection operation. `self.ptr` was returned by `VirtualAllocEx`
        // on this same process.
        unsafe {
            let _ = VirtualFreeEx(self.process, self.ptr, 0, MEM_RELEASE);
        }
    }
}

/// Inject a DLL into a target process.
///
/// # Errors
/// Returns an error if any Win32 API call fails (insufficient privileges,
/// invalid PID, etc.)
pub fn inject_dll(pid: u32, dll_path: &str) -> Result<(), HostError> {
    // Hard gate: refuse to inject if the DLL is already loaded in the target.
    let dll_filename = std::path::Path::new(dll_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("omni_overlay.dll");

    if win32::has_module(pid, dll_filename).unwrap_or(false) {
        info!(pid, dll_filename, "DLL already loaded in target — skipping injection");
        return Ok(());
    }

    // Convert DLL path to wide string (UTF-16) with null terminator
    let wide_path: Vec<u16> = OsStr::new(dll_path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path_byte_size = wide_path.len() * std::mem::size_of::<u16>();

    debug!(pid, dll_path, path_byte_size, "Opening target process");

    // SAFETY: OpenProcess with PROCESS_ALL_ACCESS on a valid PID.
    // Handle is wrapped in OwnedHandle for automatic cleanup.
    let process = OwnedHandle::new(unsafe {
        OpenProcess(PROCESS_ALL_ACCESS, false, pid)?
    });

    // SAFETY: Allocating read/write memory in the target process for the DLL path.
    // The process handle is valid (just opened above).
    let remote_mem = unsafe {
        VirtualAllocEx(
            process.raw(),
            None,
            path_byte_size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        )
    };

    if remote_mem.is_null() {
        return Err("VirtualAllocEx failed — could not allocate memory in target process".into());
    }

    // Wrap in RAII guard — freed automatically on error or after use.
    let alloc = RemoteAlloc { process: process.raw(), ptr: remote_mem };

    debug!(?remote_mem, "Allocated memory in target process");

    // SAFETY: Writing the wide-string DLL path into the allocated region.
    // `remote_mem` is a valid pointer in the target process (just allocated).
    // `wide_path` is a valid UTF-16 buffer. `path_byte_size` matches the allocation.
    unsafe {
        WriteProcessMemory(
            process.raw(),
            remote_mem,
            wide_path.as_ptr() as *const _,
            path_byte_size,
            None,
        )
    }.map_err(|e| HostError::Message(format!("WriteProcessMemory failed: {e}")))?;

    debug!("Wrote DLL path to target process memory");

    // SAFETY: kernel32.dll is loaded at the same base address in every process
    // (ASLR applies per-boot, but the address is consistent within a session).
    let kernel32 = unsafe { GetModuleHandleW(w!("kernel32.dll"))? };
    let load_library_addr = unsafe { GetProcAddress(kernel32, s!("LoadLibraryW")) }
        .ok_or(HostError::Message("GetProcAddress failed — could not find LoadLibraryW".into()))?;

    debug!(?load_library_addr, "Found LoadLibraryW address");

    // SAFETY: `load_library_addr` is a valid function pointer to LoadLibraryW.
    // LoadLibraryW has the signature `HMODULE LoadLibraryW(LPCWSTR)`, which matches
    // LPTHREAD_START_ROUTINE (one pointer-sized param, returns pointer-sized value).
    // `remote_mem` points to a null-terminated UTF-16 string in the target process.
    let thread = OwnedHandle::new(unsafe {
        CreateRemoteThread(
            process.raw(),
            None,                                               // default security
            0,                                                  // default stack size
            Some(std::mem::transmute(load_library_addr)),        // LoadLibraryW
            Some(remote_mem),                                   // DLL path as parameter
            0,                                                  // run immediately
            None,                                               // don't need thread ID
        )?
    });

    info!("Created remote thread — waiting for DLL to load");

    // SAFETY: Waiting for the remote thread to complete. 10s timeout.
    unsafe { WaitForSingleObject(thread.raw(), 10_000); }

    // Remote memory is freed on drop via RemoteAlloc guard.
    // OwnedHandle closes thread and process handles on drop.
    drop(alloc);

    info!("DLL injection complete");
    Ok(())
}

/// Eject the overlay DLL from a target process by calling its exported
/// `omni_shutdown` function via CreateRemoteThread.
///
/// `omni_shutdown` disables all minhook trampolines, sleeps to let in-flight
/// hook calls drain, then calls `FreeLibraryAndExitThread` to atomically
/// unload the DLL and exit the thread — no dangling vtable pointers.
pub fn eject_dll(pid: u32, dll_name: &str) -> Result<(), HostError> {
    let shutdown_addr = find_remote_export(pid, dll_name, "omni_shutdown")?
        .ok_or_else(|| HostError::Message(
            format!("'omni_shutdown' export not found in {} (pid {})", dll_name, pid)
        ))?;

    debug!(?shutdown_addr, "Found omni_shutdown address");

    // SAFETY: Opening the target process with full access for thread creation.
    let process = OwnedHandle::new(unsafe {
        OpenProcess(PROCESS_ALL_ACCESS, false, pid)?
    });

    // SAFETY: `shutdown_addr` is the resolved address of `omni_shutdown` in the
    // target process. It has the signature `u32 omni_shutdown(*mut c_void)`, which
    // matches LPTHREAD_START_ROUTINE.
    let thread = OwnedHandle::new(unsafe {
        CreateRemoteThread(
            process.raw(),
            None,
            0,
            Some(std::mem::transmute(shutdown_addr)),
            None,
            0,
            None,
        )?
    });

    info!("Created remote thread calling omni_shutdown — waiting for clean unload");

    // SAFETY: omni_shutdown sleeps 200ms then calls FreeLibraryAndExitThread,
    // so 10s is a generous timeout.
    unsafe { WaitForSingleObject(thread.raw(), 10_000); }

    // OwnedHandle closes thread and process handles on drop.

    info!("DLL ejection complete");
    Ok(())
}

/// Find the address of an exported function in a module loaded in a remote process.
///
/// Reads the PE export directory from the DLL file on disk to find the export's RVA,
/// then adds it to the module's base address in the remote process.
fn find_remote_export(
    pid: u32,
    dll_name: &str,
    export_name: &str,
) -> Result<Option<*const std::ffi::c_void>, HostError> {
    let remote_base = match win32::find_remote_module_base(pid, dll_name)? {
        Some(base) => base as usize,
        None => return Ok(None),
    };

    let dll_path = win32::find_remote_module_path(pid, dll_name)?
        .ok_or_else(|| HostError::Message(format!("Could not get path for '{}'", dll_name)))?;

    let rva = find_export_rva_from_file(&dll_path, export_name)?
        .ok_or_else(|| HostError::Message(
            format!("Export '{}' not found in '{}'", export_name, dll_path)
        ))?;

    let remote_addr = (remote_base + rva as usize) as *const std::ffi::c_void;
    Ok(Some(remote_addr))
}

/// Parse a PE file on disk and find the RVA of a named export.
fn find_export_rva_from_file(
    dll_path: &str,
    export_name: &str,
) -> Result<Option<u32>, HostError> {
    let data = std::fs::read(dll_path)?;

    // DOS header: e_lfanew at offset 0x3C.
    if data.len() < 0x40 {
        return Err("File too small for DOS header".into());
    }
    let e_lfanew = u32::from_le_bytes(data[0x3C..0x40].try_into()
        .map_err(|_| HostError::Message("Invalid DOS header".into()))?) as usize;

    // PE signature + COFF header (20 bytes) + optional header.
    let coff_start = e_lfanew + 4;
    if data.len() < coff_start + 20 {
        return Err("File too small for COFF header".into());
    }

    let optional_hdr_start = coff_start + 20;
    let magic = u16::from_le_bytes(data[optional_hdr_start..optional_hdr_start + 2].try_into()
        .map_err(|_| HostError::Message("Invalid optional header".into()))?);

    // Export directory is data directory index 0.
    let export_dir_offset = match magic {
        0x20B => optional_hdr_start + 112, // PE32+ (64-bit)
        0x10B => optional_hdr_start + 96,  // PE32 (32-bit)
        _ => return Err(format!("Unknown PE optional header magic: {:#x}", magic).into()),
    };

    if data.len() < export_dir_offset + 8 {
        return Err("File too small for export data directory".into());
    }

    let export_rva = u32::from_le_bytes(data[export_dir_offset..export_dir_offset + 4].try_into()
        .map_err(|_| HostError::Message("Invalid export RVA".into()))?) as usize;
    let export_size = u32::from_le_bytes(data[export_dir_offset + 4..export_dir_offset + 8].try_into()
        .map_err(|_| HostError::Message("Invalid export size".into()))?) as usize;

    if export_rva == 0 || export_size == 0 {
        return Ok(None); // No export directory.
    }

    // Convert RVA to file offset using section headers.
    let num_sections = u16::from_le_bytes(data[coff_start + 2..coff_start + 4].try_into()
        .map_err(|_| HostError::Message("Invalid section count".into()))?) as usize;
    let optional_hdr_size = u16::from_le_bytes(data[coff_start + 16..coff_start + 18].try_into()
        .map_err(|_| HostError::Message("Invalid optional header size".into()))?) as usize;
    let sections_start = optional_hdr_start + optional_hdr_size;

    let rva_to_offset = |rva: usize| -> Option<usize> {
        for i in 0..num_sections {
            let s = sections_start + i * 40;
            let vaddr = u32::from_le_bytes(data[s + 12..s + 16].try_into().ok()?) as usize;
            let vsize = u32::from_le_bytes(data[s + 8..s + 12].try_into().ok()?) as usize;
            let raw_ptr = u32::from_le_bytes(data[s + 20..s + 24].try_into().ok()?) as usize;
            if rva >= vaddr && rva < vaddr + vsize {
                return Some(rva - vaddr + raw_ptr);
            }
        }
        None
    };

    let export_offset = rva_to_offset(export_rva)
        .ok_or(HostError::Message("Could not map export directory RVA to file offset".into()))?;

    // Export directory table: NumberOfNames at +24, AddressOfFunctions at +28,
    // AddressOfNames at +32, AddressOfNameOrdinals at +36.
    let num_names = u32::from_le_bytes(data[export_offset + 24..export_offset + 28].try_into()
        .map_err(|_| HostError::Message("Invalid export name count".into()))?) as usize;
    let addr_of_functions_rva = u32::from_le_bytes(data[export_offset + 28..export_offset + 32].try_into()
        .map_err(|_| HostError::Message("Invalid functions RVA".into()))?) as usize;
    let addr_of_names_rva = u32::from_le_bytes(data[export_offset + 32..export_offset + 36].try_into()
        .map_err(|_| HostError::Message("Invalid names RVA".into()))?) as usize;
    let addr_of_ordinals_rva = u32::from_le_bytes(data[export_offset + 36..export_offset + 40].try_into()
        .map_err(|_| HostError::Message("Invalid ordinals RVA".into()))?) as usize;

    let names_offset = rva_to_offset(addr_of_names_rva)
        .ok_or(HostError::Message("Could not map names RVA".into()))?;
    let ordinals_offset = rva_to_offset(addr_of_ordinals_rva)
        .ok_or(HostError::Message("Could not map ordinals RVA".into()))?;
    let functions_offset = rva_to_offset(addr_of_functions_rva)
        .ok_or(HostError::Message("Could not map functions RVA".into()))?;

    for i in 0..num_names {
        let name_rva = u32::from_le_bytes(data[names_offset + i * 4..names_offset + i * 4 + 4].try_into()
            .map_err(|_| HostError::Message("Invalid export name entry".into()))?) as usize;
        let name_offset = rva_to_offset(name_rva)
            .ok_or(HostError::Message("Could not map export name RVA".into()))?;

        // Read null-terminated name.
        let name_end = data[name_offset..].iter().position(|&b| b == 0)
            .unwrap_or(0) + name_offset;
        let name = std::str::from_utf8(&data[name_offset..name_end])
            .map_err(|e| HostError::Message(format!("Invalid UTF-8 in export name: {e}")))?;

        if name == export_name {
            let ordinal = u16::from_le_bytes(data[ordinals_offset + i * 2..ordinals_offset + i * 2 + 2].try_into()
                .map_err(|_| HostError::Message("Invalid ordinal entry".into()))?) as usize;
            let func_rva = u32::from_le_bytes(data[functions_offset + ordinal * 4..functions_offset + ordinal * 4 + 4].try_into()
                .map_err(|_| HostError::Message("Invalid function RVA entry".into()))?);
            return Ok(Some(func_rva));
        }
    }

    Ok(None)
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/injector/mod.rs
git commit -m "refactor(host): injector uses win32:: helpers + RemoteAlloc RAII guard"
```

---

### Task 6: Convert remaining host modules to `HostError`

**Files:**
- Modify: `host/src/ipc/mod.rs`
- Modify: `host/src/watcher.rs`

- [ ] **Step 1: Convert `ipc/mod.rs` to use `HostError`**

In `host/src/ipc/mod.rs`, change the `create()` signature from `Result<Self, String>` to `Result<Self, crate::error::HostError>` and replace string errors:

Line 25, change:
```rust
    pub fn create() -> Result<Self, String> {
```
To:
```rust
    pub fn create() -> Result<Self, crate::error::HostError> {
```

Line 39, change:
```rust
            .map_err(|e| format!("CreateFileMappingW failed: {e}"))?
```
To:
```rust
            .map_err(crate::error::HostError::Win32)?
```

Line 48, change:
```rust
            return Err("MapViewOfFile returned null".into());
```
To:
```rust
            return Err(crate::error::HostError::Message("MapViewOfFile returned null".into()));
```

Add SAFETY comments to all unsafe blocks in this file:

Before `CreateFileMappingW` (line 31):
```rust
        // SAFETY: INVALID_HANDLE_VALUE (-1) creates a page-file-backed mapping.
        // name_wide is a valid null-terminated UTF-16 string. Size is computed
        // from the known layout of SharedOverlayState.
```

Before `MapViewOfFile` (line 42):
```rust
        // SAFETY: `handle` was successfully created above. FILE_MAP_ALL_ACCESS
        // grants read/write. Offset 0, size 0 means map the entire region.
```

Before `ptr::write_bytes` (line 54):
```rust
        // SAFETY: `state_ptr` points to a freshly mapped region of size
        // `size_of::<SharedOverlayState>()`. write_bytes zeroes it completely.
        // The subsequent AtomicU64::new(0) initializes the atomic field properly.
```

Before the `write()` method's unsafe blocks (lines 71-73):
```rust
        // SAFETY: `self.ptr` is valid for the lifetime of this writer (mapped
        // in `create`, unmapped in `Drop`). Only the host thread calls `write`,
        // so no concurrent mutation of the writer slot.
```

Before the `read_dll_frame_data` unsafe block (line 96):
```rust
        // SAFETY: `self.ptr` is valid. Reading `dll_frame_data` is a plain
        // Copy read — no synchronization needed beyond the atomic slot flip.
```

Before the `Drop` impl unsafe block (line 103):
```rust
        // SAFETY: Unmapping and closing the handle we created in `create`.
        // No other code accesses these after drop.
```

- [ ] **Step 2: Convert `watcher.rs` to use `HostError`**

In `host/src/watcher.rs`, change the `start()` signature from `Result<Self, String>` to `Result<Self, crate::error::HostError>`:

Line 50, change:
```rust
    ) -> Result<Self, String> {
```
To:
```rust
    ) -> Result<Self, crate::error::HostError> {
```

The existing `.map_err(|e| format!(...))` calls will work because `HostError` implements `From<String>`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/ipc/mod.rs host/src/watcher.rs
git commit -m "refactor(host): ipc and watcher use HostError, add SAFETY comments to ipc"
```

---

### Task 7: Modernize `LazyLock` pattern in `expression.rs`

**Files:**
- Modify: `host/src/omni/expression.rs:16-28`

- [ ] **Step 1: Replace `Mutex<Option<HashSet>>` with `LazyLock<Mutex<HashSet>>`**

In `host/src/omni/expression.rs`, replace lines 16-28:

```rust
use omni_shared::SensorSnapshot;
use std::sync::Mutex;
use std::collections::HashSet;

static WARNED_EXPRS: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn warn_once(expr: &str, reason: &str) {
    let mut guard = WARNED_EXPRS.lock().unwrap_or_else(|e| e.into_inner());
    let set = guard.get_or_insert_with(HashSet::new);
    if set.insert(expr.to_string()) {
        tracing::warn!("expression eval failed for {:?}: {}", expr, reason);
    }
}
```

With:

```rust
use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use omni_shared::SensorSnapshot;

static WARNED_EXPRS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn warn_once(expr: &str, reason: &str) {
    let mut set = WARNED_EXPRS.lock().unwrap_or_else(|e| e.into_inner());
    if set.insert(expr.to_string()) {
        tracing::warn!("expression eval failed for {:?}: {}", expr, reason);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/omni/expression.rs
git commit -m "refactor(host): modernize WARNED_EXPRS to LazyLock<Mutex<HashSet>>"
```

---

### Task 8: Import ordering across host crate

**Files:**
- Modify: `host/src/omni/resolver.rs:1-26`
- Modify: `host/src/main.rs:1-6`
- Modify: `host/src/ws_server.rs:7-16`
- Modify: `host/src/sensors/mod.rs:1-13`

- [ ] **Step 1: Fix import ordering in `resolver.rs`**

In `host/src/omni/resolver.rs`, replace lines 7-26:

```rust
use std::collections::HashMap;

use omni_shared::{ComputedWidget, SensorSnapshot, WidgetType, SensorSource, write_fixed_str};
use tracing::warn;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
};
use windows::core::w;

use super::types::{OmniFile, ResolvedStyle};
use super::css;
use super::flat_tree::{self, FlatNode};
use super::interpolation;
use super::layout;
use super::sensor_map;
use super::transition;
use super::reactive;
```

With:

```rust
use std::collections::HashMap;

use omni_shared::{ComputedWidget, SensorSnapshot, WidgetType, SensorSource, write_fixed_str};
use tracing::warn;
use windows::core::w;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
};

use super::css;
use super::flat_tree::{self, FlatNode};
use super::interpolation;
use super::layout;
use super::reactive;
use super::sensor_map;
use super::transition;
use super::types::{OmniFile, ResolvedStyle};
```

- [ ] **Step 2: Fix import ordering in `main.rs`**

In `host/src/main.rs`, reorder the top imports (lines 1-6) to group std, then external, then local:

```rust
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tracing::{info, warn, error, debug};
use tracing_subscriber::EnvFilter;
```

- [ ] **Step 3: Run check**

Run: `cargo check -p omni-host`
Expected: Compiles with no new errors.

- [ ] **Step 4: Commit**

```bash
git add host/src/omni/resolver.rs host/src/main.rs host/src/ws_server.rs host/src/sensors/mod.rs
git commit -m "cleanup(host): standardize import ordering (std / external / local)"
```

---

### Task 9: Add SAFETY comments to remaining host unsafe blocks

**Files:**
- Modify: `host/src/sensors/gpu.rs`
- Modify: `host/src/omni/resolver.rs`
- Modify: `host/src/main.rs`

- [ ] **Step 1: Add SAFETY comments to `gpu.rs`**

In `host/src/sensors/gpu.rs`, add SAFETY comments to the two main unsafe blocks:

Before `init_nvml` body (line 78):
```rust
    /// # Safety
    /// All function pointers are resolved from nvml.dll via `GetProcAddress`.
    /// If any pointer is null, `GetProcAddress` returns `None` and we
    /// propagate via `?`. The NVML API is stable and documented by NVIDIA.
    unsafe fn init_nvml() -> Option<Self> {
```

Before the `poll()` unsafe block (line 144):
```rust
        // SAFETY: All function pointers were validated during `init_nvml`.
        // `self.device` is a valid NVML device handle obtained via
        // `nvmlDeviceGetHandleByIndex`. Each NVML call writes to a
        // stack-local out-parameter of the correct type.
        unsafe {
```

- [ ] **Step 2: Add SAFETY comment to `resolver.rs` DirectWrite factory creation**

Find the `DWriteCreateFactory` call in `resolver.rs` and add a SAFETY comment:

```rust
        // SAFETY: DWriteCreateFactory is safe to call from any thread.
        // DWRITE_FACTORY_TYPE_SHARED returns a process-wide singleton.
```

- [ ] **Step 3: Add SAFETY comment to `main.rs` `run_stop` process termination**

In `host/src/main.rs` `run_stop()` function, add a SAFETY comment before the unsafe block (around line 291):

```rust
        // SAFETY: Opening with PROCESS_TERMINATE and calling TerminateProcess
        // on a verified omni-host.exe PID. Handle is closed immediately after.
        unsafe {
```

- [ ] **Step 4: Run check**

Run: `cargo check -p omni-host`
Expected: Compiles clean.

- [ ] **Step 5: Commit**

```bash
git add host/src/sensors/gpu.rs host/src/omni/resolver.rs host/src/main.rs
git commit -m "docs(host): add SAFETY comments to gpu, resolver, and main unsafe blocks"
```

---

## Phase 3: omni-overlay-dll

### Task 10: Add SAFETY comments to DLL crate

**Files:**
- Modify: `overlay-dll/src/lib.rs`
- Modify: `overlay-dll/src/hook.rs`
- Modify: `overlay-dll/src/present.rs`
- Modify: `overlay-dll/src/renderer.rs`
- Modify: `overlay-dll/src/frame_stats.rs`
- Modify: `overlay-dll/src/ipc/mod.rs`

- [ ] **Step 1: Add SAFETY comments to `lib.rs`**

In `overlay-dll/src/lib.rs`:

Before `DLL_MODULE` static (line 16):
```rust
/// Module handle for this DLL, saved on attach for use during shutdown.
///
/// # Safety
/// Written once in `DllMain` (DLL_PROCESS_ATTACH), read once in `omni_shutdown`.
/// Both are serialized by the Windows loader / our own call sequence.
static mut DLL_MODULE: Option<HINSTANCE> = None;
```

Before `DllMain` body, add SAFETY to the inner unsafe block (line 34):
```rust
            std::thread::spawn(|| {
                // SAFETY: install_hooks is called on a dedicated thread (not
                // under loader lock). It accesses only static mut globals that
                // are not yet initialized, so no data race.
                if let Err(e) = unsafe { hook::install_hooks() } {
```

Before `omni_shutdown`'s `GetModuleHandleA` call (line 78):
```rust
    // SAFETY: GetModuleHandleA with a known module name returns a valid
    // HMODULE. FreeLibraryAndExitThread atomically unloads and exits —
    // no code from this DLL runs after this point.
```

- [ ] **Step 2: Add SAFETY comments to `hook.rs`**

In `overlay-dll/src/hook.rs`, add SAFETY comments to:

The `static mut` globals (lines 63, 76, 85) — add a block comment before the group:
```rust
// SAFETY (static mut globals): These are written once during `install_hooks`
// (single init thread) and read from hook callbacks on the render thread.
// The `HOOKS_INSTALLED` AtomicBool gate ensures hooks are fully installed
// before any callback fires. On shutdown, hooks are disabled (SeqCst)
// before any globals are cleared.
pub static mut CAPTURED_COMMAND_QUEUE: Option<ID3D12CommandQueue> = None;
```

The `unsafe impl Send/Sync for SwapChainVtable` (lines 96-97):
```rust
// SAFETY: SwapChainVtable contains raw pointers to vtable entries in dxgi.dll.
// We only store and compare these addresses — never dereference them as COM objects.
// The addresses are valid for the lifetime of dxgi.dll (the entire process).
unsafe impl Send for SwapChainVtable {}
unsafe impl Sync for SwapChainVtable {}
```

Before `read_vtable` (line 147):
```rust
/// # Safety
/// `swap_chain` must be a valid IDXGISwapChain1 with a live COM reference.
/// The vtable pointer is dereferenced to read function addresses.
unsafe fn read_vtable(swap_chain: &IDXGISwapChain1) -> SwapChainVtable {
```

Before `create_dummy_window` (line 101):
```rust
/// # Safety
/// Calls Win32 window registration and creation APIs. The returned HWND
/// must be destroyed by the caller via `DestroyWindow`.
unsafe fn create_dummy_window() -> Result<HWND, String> {
```

Before `hooked_create_swap_chain_for_hwnd` transmute (line 330):
```rust
    // SAFETY: `p_device` is a valid IUnknown pointer provided by the DXGI runtime.
    // We attempt a QueryInterface for ID3D12CommandQueue — if the game is DX11,
    // this returns E_NOINTERFACE and we skip it.
```

Before each `minhook::MinHook::create_hook` call in `install_hooks`, add:
```rust
    // SAFETY: `vtable.present` is a valid function address read from the
    // IDXGISwapChain vtable. `hooked_present` has the same calling convention
    // and signature. MinHook patches the target in-place and returns the
    // original function pointer (trampoline).
```

- [ ] **Step 3: Add SAFETY comments to `present.rs`**

In `overlay-dll/src/present.rs`:

Before the `static mut` group (lines 14-22):
```rust
// SAFETY (static mut globals): All accessed exclusively from the render thread
// (the thread that calls Present). `ensure_renderer` and `ensure_shm_reader`
// are only called from `render_overlay`, which is only called from the hooked
// Present/Present1 functions — always on the game's render thread.
// `RENDERER_INIT_DONE` (AtomicBool) gates first-time init. `destroy_renderer`
// is called from `omni_shutdown` after hooks are disabled and drained (200ms sleep).
```

Before `ensure_renderer` unsafe accesses (line 30):
```rust
    // SAFETY: Called only from `render_overlay` on the render thread.
    // RENDERER_INIT_DONE atomic ensures one-time init. After init,
    // RENDERER is either Some (success) or None (failed) — stable.
```

Before `render_overlay` (line 57):
```rust
/// # Safety
/// Must be called on the game's render thread (from a hooked Present call).
/// `swap_chain` must be a valid IDXGISwapChain pointer from the DXGI runtime.
```

Before `destroy_renderer` (line 150):
```rust
/// # Safety
/// Must be called after all hooks are disabled and drained (no in-flight
/// render calls). Called from `omni_shutdown` which sleeps 200ms after
/// disabling hooks.
```

Before each `hooked_present`, `hooked_present1`, `hooked_resize_buffers`:
```rust
/// # Safety
/// Called by the DXGI runtime via minhook trampoline. `swap_chain` is the
/// same pointer the game passed to the original Present function.
```

- [ ] **Step 4: Add SAFETY comments to `frame_stats.rs`**

In `overlay-dll/src/frame_stats.rs`, before the `QueryPerformanceFrequency` call (line 47):
```rust
        // SAFETY: QueryPerformanceFrequency always succeeds on Windows XP+.
        // Writes to a stack-local i64.
        let mut freq: i64 = 0;
        unsafe {
            let _ = QueryPerformanceFrequency(&mut freq);
        }
```

Find the `QueryPerformanceCounter` call in `record()` and add:
```rust
        // SAFETY: QueryPerformanceCounter always succeeds on Windows XP+.
        // Writes to a stack-local i64.
```

- [ ] **Step 5: Add SAFETY comments to `ipc/mod.rs` (DLL side)**

In `overlay-dll/src/ipc/mod.rs`:

Before `unsafe impl Send/Sync` (lines 17-18):
```rust
// SAFETY: SharedMemoryReader is only used on the render thread. Send+Sync
// are required because it's stored in a `static mut` (present.rs), but
// actual access is single-threaded.
```

Before `OpenFileMappingW` call (line 27):
```rust
            // SAFETY: Opening an existing named file mapping created by the host.
            // name_wide is a valid null-terminated UTF-16 string on the stack.
```

Before `MapViewOfFile` call (line 39):
```rust
            // SAFETY: handle was successfully opened above. FILE_MAP_ALL_ACCESS
            // matches the host's PAGE_READWRITE protection. Size 0 maps the full region.
```

Before `read` and `read_current` dereferences:
```rust
        // SAFETY: `self.ptr` points to valid shared memory mapped in `open`.
        // The host writes to the inactive slot and atomically flips — we read
        // from the active slot, so no torn reads.
```

Before `write_frame_data`:
```rust
    /// # Safety
    /// Caller must ensure no concurrent writes to `dll_frame_data` from
    /// another thread. In practice, only the render thread calls this.
```

Before `Drop` impl:
```rust
        // SAFETY: Unmapping the view and closing the handle we opened in `open`.
        // After drop, `self.ptr` is never accessed again.
```

- [ ] **Step 6: Fix import ordering in `renderer.rs`**

In `overlay-dll/src/renderer.rs`, the imports are already grouped but the `omni_shared` import (line 37) should move to the external crates group. Reorder to:

```rust
use std::ffi::c_void;
use std::mem::ManuallyDrop;

use omni_shared::{ComputedWidget, read_fixed_str};
use windows::core::{w, Interface, IUnknown};
use windows::Win32::Graphics::Direct2D::Common::{ ... };
use windows::Win32::Graphics::Direct2D::{ ... };
use windows::Win32::Graphics::Direct3D11::{ ... };
use windows::Win32::Graphics::Direct3D11on12::{ ... };
use windows::Win32::Graphics::Direct3D12::{ ... };
use windows::Win32::Graphics::DirectWrite::{ ... };
use windows::Win32::Graphics::Dxgi::{ ... };
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;

use crate::logging::log_to_file;
```

- [ ] **Step 7: Run check**

Run: `cargo check -p omni-overlay-dll`
Expected: Compiles clean.

- [ ] **Step 8: Commit**

```bash
git add overlay-dll/src/
git commit -m "docs(dll): add SAFETY comments to all unsafe blocks, fix import ordering"
```

---

## Verification

### Task 11: Full build and test verification

- [ ] **Step 1: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass across all three crates.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace --all-targets`
Expected: No new warnings introduced.

- [ ] **Step 3: Run fmt check**

Run: `cargo fmt --all -- --check`
Expected: No formatting issues, or run `cargo fmt --all` to fix.

- [ ] **Step 4: Final commit if fmt made changes**

```bash
git add -A
git commit -m "cleanup: cargo fmt"
```
