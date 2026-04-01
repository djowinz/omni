# Production Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix correctness bugs (torn reads, unsound static mut), reduce attack surface (over-privileged handles), harden PE parsing, improve efficiency (idle CPU), and make the shared memory protocol version-safe — all before first public release.

**Architecture:** Eight independent fixes, ordered by severity: memory model correctness first, then soundness, then security/robustness, then structural improvements. Each task produces a working, testable commit.

**Tech Stack:** Rust 1.80+, `windows` crate v0.58, `omni-shared` (#[repr(C)] FFI types), `minhook` v0.9

---

## File Structure

### Modified Files
- `shared/src/ipc_protocol.rs` — add version field, fences documentation
- `host/src/ipc/mod.rs` — add Release fence before flip, version check, shared-mem-name with PID
- `overlay-dll/src/ipc/mod.rs` — add Acquire fence after slot read, version check, shared-mem-name with PID
- `overlay-dll/src/present.rs` — replace `static mut` with `SingleThread<RenderState>`
- `overlay-dll/src/hook.rs` — replace `static mut` with `SingleThread<HookState>`
- `overlay-dll/src/lib.rs` — replace `static mut DLL_MODULE` with `SingleThread`
- `host/src/main.rs` — extract `HostState` struct, event-driven main loop
- `host/src/injector/mod.rs` — minimum process rights, `read_u32`/`read_u16` PE helpers
- `host/src/win32.rs` — add `wchar_eq_ignore_ascii_case` zero-alloc comparison

---

## Task 1: Fix IPC torn-read bug with memory fences

**Files:**
- Modify: `host/src/ipc/mod.rs:76-101`
- Modify: `overlay-dll/src/ipc/mod.rs:66-88`

- [ ] **Step 1: Add Release fence in host writer before flip**

In `host/src/ipc/mod.rs`, in the `write()` method, add a `fence(Release)` between writing slot data and calling `flip_slot()`. Change lines 82-101:

```rust
    pub fn write(
        &mut self,
        sensor_data: &SensorSnapshot,
        widgets: &[ComputedWidget],
        layout_version: u64,
    ) {
        // SAFETY: self.ptr is valid for the lifetime of this writer.
        // Only the host thread calls write.
        let state = unsafe { &*self.ptr };
        let slot_idx = state.writer_slot_index();
        let slot = unsafe { &mut (*self.ptr).slots[slot_idx] };

        self.sequence += 1;
        slot.write_sequence = self.sequence;
        slot.sensor_data = *sensor_data;
        slot.layout_version = layout_version;

        let count = widgets.len().min(MAX_WIDGETS);
        slot.widget_count = count as u32;
        slot.widgets[..count].copy_from_slice(&widgets[..count]);

        for w in &mut slot.widgets[count..] {
            *w = ComputedWidget::default();
        }

        // Ensure all slot writes are visible before the index flip.
        // On x86 this compiles to nothing (strong memory model), but
        // it is required for correctness under the C++ memory model
        // and on ARM architectures.
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);

        state.flip_slot();
    }
```

- [ ] **Step 2: Add Acquire fence in DLL reader after loading slot index**

In `overlay-dll/src/ipc/mod.rs`, in `read_current()`, add a fence after loading the slot index:

```rust
    pub fn read_current(&self) -> &OverlaySlot {
        // SAFETY: self.ptr points to valid shared memory mapped in open.
        let state = unsafe { &*self.ptr };
        let slot_idx = state.reader_slot_index();

        // Ensure the slot index load completes before reading slot data.
        // Pairs with the Release fence in the host's write() method.
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);

        &state.slots[slot_idx]
    }
```

Also update the `read()` method the same way:

```rust
    pub fn read(&mut self) -> Option<&OverlaySlot> {
        // SAFETY: self.ptr points to valid shared memory mapped in open.
        let state = unsafe { &*self.ptr };
        let slot_idx = state.reader_slot_index();

        // Pairs with the Release fence in the host's write() method.
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);

        let slot = &state.slots[slot_idx];

        if slot.write_sequence == self.last_sequence {
            return None;
        }

        self.last_sequence = slot.write_sequence;
        Some(slot)
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/ipc/mod.rs overlay-dll/src/ipc/mod.rs
git commit -m "fix(ipc): add memory fences to prevent torn reads on non-x86 architectures"
```

---

## Task 2: Replace `static mut` with `UnsafeCell` wrapper in DLL

**Files:**
- Modify: `overlay-dll/src/present.rs`
- Modify: `overlay-dll/src/hook.rs`
- Modify: `overlay-dll/src/lib.rs`

- [ ] **Step 1: Create the `SingleThread<T>` wrapper and `RenderState` struct in `present.rs`**

Replace lines 14-28 of `overlay-dll/src/present.rs`:

```rust
use std::cell::UnsafeCell;

/// Wrapper for state that is only accessed from a single thread.
/// We implement Sync so it can be stored in a `static`, but the caller
/// must guarantee no concurrent access.
pub struct SingleThread<T>(pub UnsafeCell<T>);
// SAFETY: Access is single-threaded — see per-site comments.
unsafe impl<T> Sync for SingleThread<T> {}

/// All mutable render-thread state, grouped to avoid aliasing concerns
/// from multiple `static mut` declarations.
pub struct RenderState {
    pub renderer: Option<OverlayRenderer>,
    pub shm_reader: Option<SharedMemoryReader>,
    pub frame_stats: Option<crate::frame_stats::FrameStats>,
    pub original_present: Option<PresentFn>,
    pub original_present1: Option<Present1Fn>,
    pub original_resize_buffers: Option<ResizeBuffersFn>,
}

static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);
pub static RENDERER_INIT_DONE: AtomicBool = AtomicBool::new(false);

pub static RENDER_STATE: SingleThread<RenderState> = SingleThread(UnsafeCell::new(RenderState {
    renderer: None,
    shm_reader: None,
    frame_stats: None,
    original_present: None,
    original_present1: None,
    original_resize_buffers: None,
}));
```

Then update all functions in `present.rs` that previously accessed `static mut RENDERER`, `SHM_READER`, `FRAME_STATS`, `ORIGINAL_PRESENT`, `ORIGINAL_PRESENT1`, `ORIGINAL_RESIZE_BUFFERS` to instead get `&mut *RENDER_STATE.0.get()` once at the top and access fields through it.

For example, `ensure_renderer`:
```rust
unsafe fn ensure_renderer(state: &mut RenderState) {
    if RENDERER_INIT_DONE.load(Ordering::Acquire) {
        return;
    }
    match OverlayRenderer::init() {
        Ok(r) => {
            state.renderer = Some(r);
            RENDERER_INIT_DONE.store(true, Ordering::Release);
            state.frame_stats = Some(crate::frame_stats::FrameStats::new());
            log_to_file("[present] frame stats initialized");
            log_to_file("[present] D2D renderer initialized on first frame");
        }
        Err(e) => {
            log_to_file(&format!("[present] FATAL: renderer init failed: {e}"));
            RENDERER_INIT_DONE.store(true, Ordering::Release);
        }
    }
}
```

Each hook function (`hooked_present`, `hooked_present1`, `hooked_resize_buffers`) gets the state at entry:
```rust
pub unsafe extern "system" fn hooked_present(
    swap_chain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let state = &mut *RENDER_STATE.0.get();
    // ... use state.original_present, etc.
}
```

`destroy_renderer` similarly takes `&mut *RENDER_STATE.0.get()`.

- [ ] **Step 2: Update `hook.rs` to use `SingleThread` for hook state**

In `overlay-dll/src/hook.rs`, replace the three `static mut` globals for hook originals and captured command queue. Since these are written on the init thread and read from hook callbacks (different threads), they need `AtomicPtr` or the init thread must finish before hooks fire (which `HOOKS_INSTALLED` already guarantees).

Keep `CAPTURED_COMMAND_QUEUE` and `ORIGINAL_CREATE_SWAP_CHAIN_FOR_HWND` and `ORIGINAL_EXECUTE_COMMAND_LISTS` as `static mut` but wrapped in a `SingleThread<HookOriginals>`:

```rust
pub struct HookOriginals {
    pub captured_command_queue: Option<ID3D12CommandQueue>,
    pub original_create_swap_chain_for_hwnd: Option<CreateSwapChainForHwndFn>,
    pub original_execute_command_lists: Option<ExecuteCommandListsFn>,
}

pub static HOOK_STATE: SingleThread<HookOriginals> = SingleThread(UnsafeCell::new(HookOriginals {
    captured_command_queue: None,
    original_create_swap_chain_for_hwnd: None,
    original_execute_command_lists: None,
}));
```

Import `SingleThread` from `crate::present::SingleThread` (or move `SingleThread` to a small `util.rs` module and use it from both).

Update `install_hooks` and the hooked functions to access `HOOK_STATE` instead of bare `static mut`.

- [ ] **Step 3: Update `lib.rs` for `DLL_MODULE`**

Replace `static mut DLL_MODULE: Option<HINSTANCE> = None;` with:

```rust
static DLL_MODULE: SingleThread<Option<HINSTANCE>> = SingleThread(UnsafeCell::new(None));
```

Update `DllMain` and `omni_shutdown` to use `&mut *DLL_MODULE.0.get()`.

- [ ] **Step 4: Run tests and check**

Run: `cargo check -p omni-overlay-dll`
Run: `cargo test --workspace`
Expected: Compiles and all tests pass.

- [ ] **Step 5: Commit**

```bash
git add overlay-dll/src/present.rs overlay-dll/src/hook.rs overlay-dll/src/lib.rs
git commit -m "fix(dll): replace static mut with UnsafeCell wrapper to prevent aliasing UB"
```

---

## Task 3: Add shared memory version field

**Files:**
- Modify: `shared/src/ipc_protocol.rs`
- Modify: `host/src/ipc/mod.rs`
- Modify: `overlay-dll/src/ipc/mod.rs`

- [ ] **Step 1: Add version constant and field to `SharedOverlayState`**

In `shared/src/ipc_protocol.rs`, add a version constant and field:

```rust
/// Protocol version. Bump when SharedOverlayState layout changes.
/// Host writes this on creation; DLL checks it on open.
pub const IPC_PROTOCOL_VERSION: u32 = 1;

#[repr(C)]
pub struct SharedOverlayState {
    /// Protocol version — must match IPC_PROTOCOL_VERSION on both sides.
    pub version: u32,
    /// 0 or 1 — which slot the DLL should read from.
    pub active_slot: AtomicU64,
    pub slots: [OverlaySlot; 2],
    pub dll_frame_data: crate::sensor_types::FrameData,
}
```

- [ ] **Step 2: Write version in host on creation**

In `host/src/ipc/mod.rs`, after zeroing the shared memory, set the version:

```rust
        unsafe {
            ptr::write_bytes(state_ptr, 0, 1);
            (*state_ptr).version = omni_shared::IPC_PROTOCOL_VERSION;
            (*state_ptr).active_slot = std::sync::atomic::AtomicU64::new(0);
        }
```

- [ ] **Step 3: Check version in DLL on open**

In `overlay-dll/src/ipc/mod.rs`, after mapping the view, check the version before returning:

```rust
        let state = unsafe { &*ptr_cast };
        if state.version != omni_shared::IPC_PROTOCOL_VERSION {
            log_to_file(&format!(
                "[ipc] version mismatch: expected {}, found {}",
                omni_shared::IPC_PROTOCOL_VERSION, state.version
            ));
            unsafe { let _ = CloseHandle(handle); }
            return None;
        }
```

- [ ] **Step 4: Update the size stability test**

In `shared/src/ipc_protocol.rs` tests, the `shared_state_size_is_stable` test should still pass since we're adding a `u32` field — the size will increase by 4 bytes (plus potential alignment padding). Update the assertion threshold if needed.

- [ ] **Step 5: Run tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add shared/src/ipc_protocol.rs host/src/ipc/mod.rs overlay-dll/src/ipc/mod.rs
git commit -m "feat(ipc): add protocol version field to prevent host/DLL layout mismatch"
```

---

## Task 4: Use minimum process access rights in injector

**Files:**
- Modify: `host/src/injector/mod.rs`

- [ ] **Step 1: Replace `PROCESS_ALL_ACCESS` with minimum required rights**

In `host/src/injector/mod.rs`, add the specific access rights import and replace usage.

For `inject_dll`, replace `PROCESS_ALL_ACCESS` with the minimum needed:

```rust
use windows::Win32::System::Threading::{
    CreateRemoteThread, OpenProcess, WaitForSingleObject,
    PROCESS_CREATE_THREAD, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_WRITE,
};
```

In `inject_dll`:
```rust
    const INJECT_ACCESS: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(
        PROCESS_CREATE_THREAD.0
            | PROCESS_VM_OPERATION.0
            | PROCESS_VM_WRITE.0
            | PROCESS_QUERY_INFORMATION.0,
    );
    let process = OwnedHandle::new(unsafe { OpenProcess(INJECT_ACCESS, false, pid)? });
```

For `eject_dll`, it only needs to create a thread:
```rust
    const EJECT_ACCESS: PROCESS_ACCESS_RIGHTS = PROCESS_ACCESS_RIGHTS(
        PROCESS_CREATE_THREAD.0 | PROCESS_QUERY_INFORMATION.0,
    );
    let process = OwnedHandle::new(unsafe { OpenProcess(EJECT_ACCESS, false, pid)? });
```

Also add `use windows::Win32::System::Threading::PROCESS_ACCESS_RIGHTS;` to the imports.

- [ ] **Step 2: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 3: Commit**

```bash
git add host/src/injector/mod.rs
git commit -m "fix(injector): use minimum required process access rights instead of PROCESS_ALL_ACCESS"
```

---

## Task 5: Harden PE parser with bounds-checked read helpers

**Files:**
- Modify: `host/src/injector/mod.rs`

- [ ] **Step 1: Add `read_u16` and `read_u32` helpers**

Add these at the bottom of `host/src/injector/mod.rs`, before the last closing brace:

```rust
/// Read a little-endian u32 from `data` at `offset` with bounds checking.
fn read_u32(data: &[u8], offset: usize) -> Result<u32, HostError> {
    data.get(offset..offset + 4)
        .and_then(|s| s.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| HostError::Message(format!("PE read out of bounds at offset {offset:#x}")))
}

/// Read a little-endian u16 from `data` at `offset` with bounds checking.
fn read_u16(data: &[u8], offset: usize) -> Result<u16, HostError> {
    data.get(offset..offset + 2)
        .and_then(|s| s.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or_else(|| HostError::Message(format!("PE read out of bounds at offset {offset:#x}")))
}
```

- [ ] **Step 2: Rewrite `find_export_rva_from_file` using the helpers**

Replace the entire `find_export_rva_from_file` function body to use `read_u32` and `read_u16` instead of manual slice + `try_into` + `map_err`:

```rust
fn find_export_rva_from_file(dll_path: &str, export_name: &str) -> Result<Option<u32>, HostError> {
    let data = std::fs::read(dll_path)?;

    if data.len() < 0x40 {
        return Err("File too small for DOS header".into());
    }
    let e_lfanew = read_u32(&data, 0x3C)? as usize;

    let coff_start = e_lfanew + 4;
    if data.len() < coff_start + 20 {
        return Err("File too small for COFF header".into());
    }

    let optional_hdr_start = coff_start + 20;
    let magic = read_u16(&data, optional_hdr_start)?;

    let export_dir_offset = match magic {
        0x20B => optional_hdr_start + 112,
        0x10B => optional_hdr_start + 96,
        _ => return Err(format!("Unknown PE optional header magic: {magic:#x}").into()),
    };

    let export_rva = read_u32(&data, export_dir_offset)? as usize;
    let export_size = read_u32(&data, export_dir_offset + 4)? as usize;

    if export_rva == 0 || export_size == 0 {
        return Ok(None);
    }

    let num_sections = read_u16(&data, coff_start + 2)? as usize;
    let optional_hdr_size = read_u16(&data, coff_start + 16)? as usize;
    let sections_start = optional_hdr_start + optional_hdr_size;

    let rva_to_offset = |rva: usize| -> Option<usize> {
        for i in 0..num_sections {
            let s = sections_start + i * 40;
            let vaddr = u32::from_le_bytes(data.get(s + 12..s + 16)?.try_into().ok()?) as usize;
            let vsize = u32::from_le_bytes(data.get(s + 8..s + 12)?.try_into().ok()?) as usize;
            let raw_ptr = u32::from_le_bytes(data.get(s + 20..s + 24)?.try_into().ok()?) as usize;
            if rva >= vaddr && rva < vaddr + vsize {
                return Some(rva - vaddr + raw_ptr);
            }
        }
        None
    };

    let export_offset = rva_to_offset(export_rva)
        .ok_or_else(|| HostError::Message("Could not map export directory RVA".into()))?;

    let num_names = read_u32(&data, export_offset + 24)? as usize;
    let addr_of_functions_rva = read_u32(&data, export_offset + 28)? as usize;
    let addr_of_names_rva = read_u32(&data, export_offset + 32)? as usize;
    let addr_of_ordinals_rva = read_u32(&data, export_offset + 36)? as usize;

    let names_offset = rva_to_offset(addr_of_names_rva)
        .ok_or_else(|| HostError::Message("Could not map names RVA".into()))?;
    let ordinals_offset = rva_to_offset(addr_of_ordinals_rva)
        .ok_or_else(|| HostError::Message("Could not map ordinals RVA".into()))?;
    let functions_offset = rva_to_offset(addr_of_functions_rva)
        .ok_or_else(|| HostError::Message("Could not map functions RVA".into()))?;

    for i in 0..num_names {
        let name_rva = read_u32(&data, names_offset + i * 4)? as usize;
        let name_offset = rva_to_offset(name_rva)
            .ok_or_else(|| HostError::Message("Could not map export name RVA".into()))?;

        let name_end = data.get(name_offset..)
            .and_then(|s| s.iter().position(|&b| b == 0))
            .map(|pos| name_offset + pos)
            .unwrap_or(name_offset);
        let name = std::str::from_utf8(data.get(name_offset..name_end).unwrap_or(&[]))
            .map_err(|e| HostError::Message(format!("Invalid UTF-8 in export name: {e}")))?;

        if name == export_name {
            let ordinal = read_u16(&data, ordinals_offset + i * 2)? as usize;
            let func_rva = read_u32(&data, functions_offset + ordinal * 4)?;
            return Ok(Some(func_rva));
        }
    }

    Ok(None)
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/injector/mod.rs
git commit -m "fix(injector): bounds-checked PE parser helpers prevent panics on malformed DLLs"
```

---

## Task 6: Extract `HostState` struct from `run_host`

**Files:**
- Modify: `host/src/main.rs`

- [ ] **Step 1: Extract `HostState` struct and methods**

Create a `HostState` struct that owns the mutable state currently scattered across `run_host`'s local variables, and move `reload_theme`, `reload_overlay`, and `switch_overlay` into `impl HostState` methods:

```rust
struct HostState {
    omni_file: omni::OmniFile,
    omni_resolver: omni::resolver::OmniResolver,
    layout_version: u64,
    current_overlay: String,
    file_watcher: Option<watcher::FileWatcher>,
    data_dir: PathBuf,
    config_path: PathBuf,
}

impl HostState {
    fn new(overlay_name: String, data_dir: PathBuf, config_path: PathBuf) -> Self {
        Self {
            omni_file: omni::OmniFile::empty(),
            omni_resolver: omni::resolver::OmniResolver::new(),
            layout_version: 0,
            current_overlay: overlay_name,
            file_watcher: None,
            data_dir,
            config_path,
        }
    }

    fn reload_theme(&mut self, theme_src: &str) {
        if let Some(theme_path) = workspace::structure::resolve_theme_path(
            &self.data_dir, &self.current_overlay, theme_src,
        ) {
            match std::fs::read_to_string(&theme_path) {
                Ok(css) => self.omni_resolver.load_theme(&css),
                Err(e) => warn!(path = %theme_path.display(), error = %e, "Failed to read theme file"),
            }
        } else {
            warn!(theme_src, "Theme file not found");
        }
    }

    fn reload_overlay(&mut self) -> bool {
        let omni_path = workspace::structure::overlay_omni_path(&self.data_dir, &self.current_overlay);
        let source = match std::fs::read_to_string(&omni_path) {
            Ok(s) => s,
            Err(e) => {
                warn!(path = %omni_path.display(), error = %e, "Failed to read overlay file");
                return false;
            }
        };

        let (parsed, diagnostics) = omni::parser::parse_omni_with_diagnostics(&source);
        for diag in &diagnostics {
            match diag.severity {
                omni::parser::Severity::Error => error!(
                    line = diag.line, col = diag.column,
                    msg = %diag.message, suggestion = ?diag.suggestion,
                    "parse error"
                ),
                omni::parser::Severity::Warning => warn!(
                    line = diag.line, col = diag.column,
                    msg = %diag.message, suggestion = ?diag.suggestion,
                    "parse warning"
                ),
            }
        }

        match parsed {
            Some(new_file) => {
                if let Some(theme_src) = &new_file.theme_src {
                    let ts = theme_src.clone();
                    self.omni_file = new_file;
                    self.reload_theme(&ts);
                } else {
                    self.omni_file = new_file;
                }
                info!(widgets = self.omni_file.widgets.len(), "Overlay loaded successfully");
                self.layout_version += 1;
                true
            }
            None => {
                warn!("Parse errors in overlay — keeping previous version");
                false
            }
        }
    }

    fn switch_overlay(&mut self, new_name: &str) -> bool {
        let new_dir = workspace::structure::overlay_dir(&self.data_dir, new_name);
        let themes_dir = self.data_dir.join("themes");

        self.file_watcher = match watcher::FileWatcher::start(
            new_dir.clone(), themes_dir, self.config_path.clone(),
        ) {
            Ok(w) => {
                info!(path = %new_dir.display(), "Recreated file watcher for new overlay");
                Some(w)
            }
            Err(e) => {
                warn!(error = %e, "Failed to recreate file watcher");
                None
            }
        };

        self.current_overlay = new_name.to_string();
        self.reload_overlay()
    }

    fn handle_watcher_event(&mut self, event: watcher::ReloadEvent, config: &mut config::Config, scanner: &scanner::Scanner) {
        match event {
            watcher::ReloadEvent::Overlay => {
                info!("Overlay file changed — reloading");
                self.reload_overlay();
            }
            watcher::ReloadEvent::Theme => {
                info!("Theme file changed — reloading");
                if let Some(theme_src) = self.omni_file.theme_src.clone() {
                    self.reload_theme(&theme_src);
                    self.layout_version += 1;
                }
            }
            watcher::ReloadEvent::Config => {
                info!("Config changed — reloading");
                let new_config = config::load_config(&self.config_path);
                let new_overlay = workspace::overlay_resolver::resolve_overlay_name(
                    scanner.last_injected_exe(),
                    &new_config.overlay_by_game,
                    &new_config.active_overlay,
                    &self.data_dir,
                );
                if new_overlay != self.current_overlay {
                    info!(from = %self.current_overlay, to = %new_overlay, "Active overlay changed — switching");
                    self.switch_overlay(&new_overlay);
                }
                *config = new_config;
            }
        }
    }
}
```

- [ ] **Step 2: Simplify `run_host` to use `HostState`**

Replace the body of `run_host` to construct a `HostState` and use its methods. The main loop body becomes much shorter — no more passing 5-7 parameters to helper functions.

Delete the free functions `reload_theme`, `reload_overlay`, `switch_overlay` that were defined before `mod` declarations.

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/main.rs
git commit -m "refactor(host): extract HostState struct, eliminate parameter sprawl in main loop"
```

---

## Task 7: Event-driven main loop (eliminate idle CPU burn)

**Files:**
- Modify: `host/src/main.rs`

- [ ] **Step 1: Replace `thread::sleep` polling with `recv_timeout`**

In `run_host`, replace the `while RUNNING` loop to use `sensor_rx.recv_timeout` as the primary blocking call:

```rust
    let mut transitions_active = false;

    while RUNNING.load(Ordering::Relaxed) {
        // Block until sensor data arrives or timeout expires.
        // When transitions are active, wake at 120Hz for smooth animation.
        // When idle, wake only on scanner poll interval or sensor data.
        let timeout = if transitions_active {
            Duration::from_millis(8) // 120Hz
        } else {
            // Wake at next scanner poll or in 100ms (for watcher events)
            let until_scan = scan_interval.saturating_sub(last_scan.elapsed());
            until_scan.min(Duration::from_millis(100))
        };

        // This blocks instead of busy-spinning when idle
        while let Ok(snapshot) = sensor_rx.try_recv() {
            latest_snapshot = snapshot;
        }
        if sensor_rx.recv_timeout(timeout).ok().is_some() {
            // Got one more — drain any additional
            // (recv_timeout returned a snapshot)
        }

        // ... rest of loop body unchanged ...

        // After resolving widgets, check if any transitions are active
        transitions_active = host_state.omni_resolver.has_active_transitions();
    }
```

Note: this requires adding a `has_active_transitions()` method to `OmniResolver` that delegates to the `TransitionManager`. Check if it already exists; if not, add a simple one.

- [ ] **Step 2: Add `has_active_transitions` to `OmniResolver` if needed**

In `host/src/omni/resolver.rs`, add:

```rust
    pub fn has_active_transitions(&self) -> bool {
        self.transition_manager.has_active()
    }
```

And in `host/src/omni/transition.rs`, add to `TransitionManager`:

```rust
    pub fn has_active(&self) -> bool {
        !self.active.is_empty()
    }
```

(Check if `active` is the field name — read the file to confirm.)

- [ ] **Step 3: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add host/src/main.rs host/src/omni/resolver.rs host/src/omni/transition.rs
git commit -m "perf(host): event-driven main loop — zero CPU when idle, 120Hz only during transitions"
```

---

## Task 8: Zero-allocation module name comparison

**Files:**
- Modify: `host/src/win32.rs`
- Modify: `host/src/scanner.rs`

- [ ] **Step 1: Add `wchar_eq_ignore_ascii_case` to `win32.rs`**

```rust
/// Compare a null-terminated UTF-16 buffer to an ASCII string, case-insensitive.
/// Returns true if they match. Does not allocate.
pub fn wchar_eq_ignore_ascii_case(buf: &[u16], ascii: &str) -> bool {
    let mut buf_iter = buf.iter().copied();
    let mut str_iter = ascii.bytes();

    loop {
        match (buf_iter.next(), str_iter.next()) {
            (Some(0), None) | (None, None) => return true,   // both ended
            (Some(w), Some(a)) => {
                // ASCII case-insensitive comparison (both must be in ASCII range)
                if w > 127 || !a.is_ascii() {
                    return false;
                }
                if (w as u8).to_ascii_lowercase() != a.to_ascii_lowercase() {
                    return false;
                }
            }
            _ => return false, // length mismatch
        }
    }
}
```

- [ ] **Step 2: Use it in `has_module`, `find_remote_module`, `find_remote_module_base`, `find_remote_module_path`**

Replace `wchar_to_string(&m.szModule).eq_ignore_ascii_case(dll_name)` with `wchar_eq_ignore_ascii_case(&m.szModule, dll_name)` in all four functions in `win32.rs`.

- [ ] **Step 3: Use it in scanner's `has_graphics_dll`**

In `host/src/scanner.rs`, update `has_graphics_dll`:

```rust
fn has_graphics_dll(modules: &[MODULEENTRY32W]) -> bool {
    modules.iter().any(|m| {
        GRAPHICS_DLLS
            .iter()
            .any(|&dll| win32::wchar_eq_ignore_ascii_case(&m.szModule, dll))
    })
}
```

And the overlay-loaded check in `poll()`:
```rust
            let overlay_loaded = modules
                .iter()
                .any(|m| win32::wchar_eq_ignore_ascii_case(&m.szModule, &self.dll_filename));
```

- [ ] **Step 4: Add tests**

```rust
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
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p omni-host`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add host/src/win32.rs host/src/scanner.rs
git commit -m "perf(host): zero-allocation module name comparison eliminates heap allocs in scanner loop"
```

---

## Final Verification

### Task 9: Full workspace verification

- [ ] **Step 1:** Run `cargo test --workspace`
- [ ] **Step 2:** Run `cargo clippy --workspace --all-targets`
- [ ] **Step 3:** Run `cargo fmt --all`
- [ ] **Step 4:** Commit any fmt changes

```bash
git add -A
git commit -m "cleanup: cargo fmt"
```
