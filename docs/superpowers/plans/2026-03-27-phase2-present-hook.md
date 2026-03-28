# Phase 2: Hook IDXGISwapChain::Present

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hook `IDXGISwapChain::Present` inside the injected overlay DLL so that our code runs every frame the game renders. For now the hook just increments a frame counter and logs periodically — no rendering yet.

**Architecture:** On DLL attach, spawn a thread that creates a dummy D3D11 device + swap chain to discover the `Present` vtable address, installs an inline hook via `minhook`, and stores the original function pointer. The hooked `Present` increments a frame counter and calls the original. A `ResizeBuffers` hook is also installed to handle window/resolution changes safely in future phases.

**Tech Stack:** Rust, `windows` crate 0.58 (Direct3D 11 + DXGI features), `minhook` crate for inline hooking, file-based logging (same as Phase 1).

**Testing notes:** Unit tests cover vtable index constants. Integration testing is manual: inject into a DX11 game, check log file for frame count messages. The dummy-device creation can be tested standalone.

**Depends on:** Phase 1 complete (workspace, shared crate, overlay-dll skeleton, host injector).

---

## File Map

```
overlay-dll/
  Cargo.toml                         # Add minhook + new windows features
  src/
    lib.rs                           # DllMain — spawn init thread on attach
    hook.rs                          # Dummy device creation, vtable discovery, hook install
    present.rs                       # Hooked Present + ResizeBuffers implementations
    logging.rs                       # log_to_file helper (extracted from lib.rs)
```

---

### Task 1: Extract Logging Helper to Its Own Module

**Files:**
- Create: `overlay-dll/src/logging.rs`
- Modify: `overlay-dll/src/lib.rs`

- [ ] **Step 1: Create overlay-dll/src/logging.rs**

```rust
use std::fs::OpenOptions;
use std::io::Write;

/// Log a message to a file in the temp directory. Intentionally simple —
/// no external dependencies. Will be replaced with structured logging later.
pub fn log_to_file(msg: &str) {
    let path = std::env::temp_dir().join("omni_overlay.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "[{timestamp}] {msg}");
    }
}
```

- [ ] **Step 2: Update overlay-dll/src/lib.rs to use the module**

```rust
use std::ffi::c_void;
use windows::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

mod logging;
mod hook;
mod present;

use logging::log_to_file;

/// DLL entry point. Called by Windows when the DLL is loaded/unloaded.
///
/// # Safety
/// This is called by the Windows loader. We must not do anything complex here —
/// no heap allocations beyond simple logging, no thread creation, no LoadLibrary calls.
/// See: https://learn.microsoft.com/en-us/windows/win32/dlls/dllmain
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        x if x == DLL_PROCESS_ATTACH => {
            log_to_file("omni overlay DLL attached to process");
        }
        x if x == DLL_PROCESS_DETACH => {
            log_to_file("omni overlay DLL detached from process");
        }
        _ => {}
    }
    TRUE
}
```

Note: `hook` and `present` modules don't exist yet — create empty placeholder files so it compiles:

Create `overlay-dll/src/hook.rs`:
```rust
// Hook installation — implemented in Task 3.
```

Create `overlay-dll/src/present.rs`:
```rust
// Hooked Present/ResizeBuffers — implemented in Task 4.
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles with no errors.

- [ ] **Step 4: Commit**

```bash
git add overlay-dll/src/logging.rs overlay-dll/src/lib.rs overlay-dll/src/hook.rs overlay-dll/src/present.rs
git commit -m "refactor(overlay-dll): extract logging helper to its own module, add hook/present stubs"
```

---

### Task 2: Add Dependencies — minhook + D3D11/DXGI Windows Features

**Files:**
- Modify: `overlay-dll/Cargo.toml`

- [ ] **Step 1: Update overlay-dll/Cargo.toml**

```toml
[package]
name = "omni-overlay-dll"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
omni-shared = { path = "../shared" }
minhook = "0.9"

[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_System_SystemServices",
    "Win32_System_LibraryLoader",
    "Win32_Graphics_Direct3D",
    "Win32_Graphics_Direct3D11",
    "Win32_Graphics_Dxgi",
    "Win32_Graphics_Dxgi_Common",
    "Win32_UI_WindowsAndMessaging",
]
```

- [ ] **Step 2: Verify dependencies resolve**

Run: `cargo build -p omni-overlay-dll`
Expected: Downloads `minhook` and new `windows` features, compiles successfully.

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/Cargo.toml Cargo.lock
git commit -m "feat(overlay-dll): add minhook and D3D11/DXGI windows feature dependencies"
```

---

### Task 3: Implement Vtable Discovery via Dummy Swap Chain

**Files:**
- Modify: `overlay-dll/src/hook.rs`

This task creates a dummy D3D11 device + swap chain, reads the `Present` and `ResizeBuffers` function pointers from the vtable, then releases everything. The function pointers are returned for the caller to hook.

- [ ] **Step 1: Implement hook.rs with dummy device creation and vtable reading**

```rust
use std::ffi::c_void;
use std::mem;
use std::ptr;

use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::D3D11CreateDeviceAndSwapChain;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_MODE_DESC, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGISwapChain, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_DISCARD, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, CS_HREDRAW, CS_VREDRAW, HMENU, WNDCLASSEXW, WS_OVERLAPPED,
    RegisterClassExW, CW_USEDEFAULT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use crate::logging::log_to_file;

/// Vtable indices for IDXGISwapChain methods.
/// IUnknown(3) + IDXGIObject(4) + IDXGIDeviceSubObject(1) = 8 base methods.
pub const PRESENT_VTABLE_INDEX: usize = 8;
pub const RESIZE_BUFFERS_VTABLE_INDEX: usize = 13;

/// Discovered function pointers from the IDXGISwapChain vtable.
pub struct SwapChainVtable {
    pub present: *const c_void,
    pub resize_buffers: *const c_void,
}

/// Creates a dummy D3D11 device + swap chain, reads the Present and ResizeBuffers
/// function pointers from the vtable, then releases everything.
///
/// # Safety
/// Must be called from a thread (not DllMain). Creates and destroys a temporary
/// hidden window.
pub unsafe fn discover_swapchain_vtable() -> Result<SwapChainVtable, String> {
    // Create a dummy hidden window — D3D11CreateDeviceAndSwapChain needs an HWND
    let hwnd = create_dummy_window()?;

    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: 2,
            Height: 2,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 1,
        OutputWindow: hwnd,
        Windowed: true.into(),
        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
        ..Default::default()
    };

    let mut swap_chain: Option<IDXGISwapChain> = None;

    let result = D3D11CreateDeviceAndSwapChain(
        None,                       // default adapter
        D3D_DRIVER_TYPE_HARDWARE,
        None,                       // no software rasterizer
        Default::default(),         // no creation flags
        None,                       // default feature levels
        7,                          // SDK version (D3D11_SDK_VERSION)
        &desc,
        Some(&mut swap_chain),
        None,                       // don't need device
        None,                       // don't need feature level
        None,                       // don't need device context
    );

    if result.is_err() {
        let _ = DestroyWindow(hwnd);
        return Err(format!("D3D11CreateDeviceAndSwapChain failed: {:?}", result));
    }

    let swap_chain = swap_chain.ok_or("D3D11CreateDeviceAndSwapChain returned None")?;

    // Read the vtable. COM objects have a pointer to a vtable as their first field.
    let swap_chain_ptr: *const c_void = mem::transmute_copy(&swap_chain);
    let vtable: *const *const c_void = *(swap_chain_ptr as *const *const *const c_void);

    let present = *vtable.add(PRESENT_VTABLE_INDEX);
    let resize_buffers = *vtable.add(RESIZE_BUFFERS_VTABLE_INDEX);

    log_to_file(&format!(
        "vtable discovery: Present={:?}, ResizeBuffers={:?}",
        present, resize_buffers
    ));

    // Release the swap chain (drop releases the COM reference)
    drop(swap_chain);

    // Destroy the dummy window
    let _ = DestroyWindow(hwnd);

    Ok(SwapChainVtable {
        present,
        resize_buffers,
    })
}

/// Create a minimal hidden window for the dummy swap chain.
unsafe fn create_dummy_window() -> Result<HWND, String> {
    let hinstance = GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW: {e}"))?;

    let class_name = windows::core::w!("OmniDummyClass");

    let wc = WNDCLASSEXW {
        cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpszClassName: class_name,
        hInstance: hinstance.into(),
        ..Default::default()
    };

    RegisterClassExW(&wc);

    let hwnd = CreateWindowExW(
        Default::default(),         // no extended style
        class_name,
        windows::core::w!("OmniDummy"),
        WS_OVERLAPPED,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        2,
        2,
        HWND::default(),            // no parent
        HMENU::default(),           // no menu
        hinstance,
        None,
    )
    .map_err(|e| format!("CreateWindowExW: {e}"))?;

    Ok(hwnd)
}

/// Install Present and ResizeBuffers hooks using minhook.
///
/// # Safety
/// Must be called exactly once, from the init thread.
pub unsafe fn install_hooks() -> Result<(), String> {
    log_to_file("discovering swap chain vtable...");

    let vtable = discover_swapchain_vtable()?;

    log_to_file("installing Present hook...");

    // Install Present hook
    let original_present = minhook::MinHook::create_hook(
        vtable.present as *mut c_void,
        crate::present::hooked_present as *mut c_void,
    )
    .map_err(|e| format!("MinHook create_hook (Present): {e:?}"))?;

    crate::present::ORIGINAL_PRESENT = Some(mem::transmute(original_present));

    log_to_file("installing ResizeBuffers hook...");

    // Install ResizeBuffers hook
    let original_resize = minhook::MinHook::create_hook(
        vtable.resize_buffers as *mut c_void,
        crate::present::hooked_resize_buffers as *mut c_void,
    )
    .map_err(|e| format!("MinHook create_hook (ResizeBuffers): {e:?}"))?;

    crate::present::ORIGINAL_RESIZE_BUFFERS = Some(mem::transmute(original_resize));

    // Enable all hooks atomically
    minhook::MinHook::enable_all_hooks()
        .map_err(|e| format!("MinHook enable_all_hooks: {e:?}"))?;

    log_to_file("all hooks installed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtable_indices_are_correct() {
        // IUnknown: 3 methods (QueryInterface, AddRef, Release)
        // IDXGIObject: 4 methods (SetPrivateData, SetPrivateDataInterface, GetPrivateData, GetParent)
        // IDXGIDeviceSubObject: 1 method (GetDevice)
        // IDXGISwapChain starts at index 8
        assert_eq!(PRESENT_VTABLE_INDEX, 8);
        // Present(8), GetBuffer(9), SetFullscreenState(10), GetFullscreenState(11),
        // GetDesc(12), ResizeBuffers(13)
        assert_eq!(RESIZE_BUFFERS_VTABLE_INDEX, 13);
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles with no errors. The `present` module references will resolve in the next task.

Note: If the `present` module stubs don't have the required symbols yet, temporarily comment out the `install_hooks` function body except the vtable discovery. Or proceed directly to Task 4 and compile after both are done.

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/src/hook.rs
git commit -m "feat(overlay-dll): vtable discovery via dummy D3D11 device and hook installation"
```

---

### Task 4: Implement Hooked Present and ResizeBuffers

**Files:**
- Modify: `overlay-dll/src/present.rs`

The hooked `Present` increments a frame counter and logs every 300 frames (~5 seconds at 60fps). The hooked `ResizeBuffers` logs and calls the original — it will be extended in later phases to release/recreate render targets.

- [ ] **Step 1: Implement present.rs**

```rust
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

use windows::core::HRESULT;

use crate::logging::log_to_file;

/// Type signature for IDXGISwapChain::Present.
/// `this` is the raw COM pointer, not the windows-rs wrapper.
pub type PresentFn = unsafe extern "system" fn(
    this: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT;

/// Type signature for IDXGISwapChain::ResizeBuffers.
pub type ResizeBuffersFn = unsafe extern "system" fn(
    this: *mut c_void,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: u32,
    swap_chain_flags: u32,
) -> HRESULT;

/// Stored by hook.rs after hook installation.
pub static mut ORIGINAL_PRESENT: Option<PresentFn> = None;
pub static mut ORIGINAL_RESIZE_BUFFERS: Option<ResizeBuffersFn> = None;

/// Frame counter — incremented every Present call.
static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);

/// Called instead of the real IDXGISwapChain::Present every frame.
///
/// # Safety
/// Called by the game's render thread via the minhook trampoline.
pub unsafe extern "system" fn hooked_present(
    this: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);

    // Log every 300 frames (~5 seconds at 60fps)
    if count % 300 == 0 {
        log_to_file(&format!("Present hook active — frame {count}"));
    }

    // Call the original Present
    if let Some(original) = ORIGINAL_PRESENT {
        original(this, sync_interval, flags)
    } else {
        HRESULT(0) // S_OK
    }
}

/// Called instead of the real IDXGISwapChain::ResizeBuffers.
/// Games call this when the window is resized or fullscreen is toggled.
///
/// # Safety
/// Called by the game's render thread via the minhook trampoline.
pub unsafe extern "system" fn hooked_resize_buffers(
    this: *mut c_void,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: u32,
    swap_chain_flags: u32,
) -> HRESULT {
    log_to_file(&format!(
        "ResizeBuffers called: {width}x{height}, buffers={buffer_count}"
    ));

    // In later phases: release any render targets / views here before the resize.

    // Call the original ResizeBuffers
    if let Some(original) = ORIGINAL_RESIZE_BUFFERS {
        original(this, buffer_count, width, height, new_format, swap_chain_flags)
    } else {
        HRESULT(0) // S_OK
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles. There may be warnings about `unsafe` statics — these are acceptable for this low-level hooking code. Address any actual errors.

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/src/present.rs
git commit -m "feat(overlay-dll): hooked Present with frame counter and ResizeBuffers passthrough"
```

---

### Task 5: Wire Up DllMain to Spawn Init Thread

**Files:**
- Modify: `overlay-dll/src/lib.rs`

Update `DllMain` to spawn a thread on `DLL_PROCESS_ATTACH` that calls `hook::install_hooks()`. The loader lock prevents us from doing anything complex inside `DllMain` itself.

- [ ] **Step 1: Update lib.rs to spawn init thread**

```rust
use std::ffi::c_void;
use windows::Win32::Foundation::{BOOL, HINSTANCE, TRUE};
use windows::Win32::System::SystemServices::{DLL_PROCESS_ATTACH, DLL_PROCESS_DETACH};

mod logging;
mod hook;
mod present;

use logging::log_to_file;

/// DLL entry point. Called by Windows when the DLL is loaded/unloaded.
///
/// # Safety
/// This is called by the Windows loader. We spawn a thread for initialization
/// because the loader lock prevents complex operations in DllMain.
#[no_mangle]
pub unsafe extern "system" fn DllMain(
    _hinst: HINSTANCE,
    reason: u32,
    _reserved: *mut c_void,
) -> BOOL {
    match reason {
        x if x == DLL_PROCESS_ATTACH => {
            log_to_file("omni overlay DLL attached — spawning init thread");
            std::thread::spawn(|| {
                if let Err(e) = unsafe { hook::install_hooks() } {
                    log_to_file(&format!("FATAL: hook installation failed: {e}"));
                }
            });
        }
        x if x == DLL_PROCESS_DETACH => {
            log_to_file("omni overlay DLL detached from process");
        }
        _ => {}
    }
    TRUE
}
```

- [ ] **Step 2: Verify full DLL compiles**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles with no errors.

- [ ] **Step 3: Run workspace tests**

Run: `cargo test --workspace`
Expected: All existing tests pass (12 shared + 1 hook vtable index test = 13 total).

- [ ] **Step 4: Commit**

```bash
git add overlay-dll/src/lib.rs
git commit -m "feat(overlay-dll): spawn init thread from DllMain to install Present hook"
```

---

### Task 6: Build Release DLL and Verify File Output

**Files:** None — build and verification only.

- [ ] **Step 1: Build release DLL**

Run: `cargo build -p omni-overlay-dll --release`
Expected: Compiles. Produces `target/release/omni_overlay_dll.dll`.

- [ ] **Step 2: Verify DLL exists**

Run: `ls -la target/release/omni_overlay_dll.dll`
Expected: File exists, size is reasonable (a few hundred KB).

- [ ] **Step 3: Build host in debug mode**

Run: `cargo build -p omni-host`
Expected: Compiles (no changes to host, just confirming workspace is healthy).

---

### Task 7: Integration Test — Inject Into a DX11 Game

This is a manual integration test. No code to write.

- [ ] **Step 1: Clear old log file**

```powershell
Remove-Item $env:TEMP\omni_overlay.log -ErrorAction SilentlyContinue
```

- [ ] **Step 2: Launch a DX11 game and find its PID**

Open any DX11 game. From PowerShell:
```powershell
Get-Process | Where-Object { $_.MainWindowTitle -ne "" } | Select-Object Id, ProcessName, MainWindowTitle
```

- [ ] **Step 3: Inject the release DLL**

From a Windows terminal (run as Administrator if the game is elevated):
```powershell
cargo run -p omni-host -- <GAME_PID> "C:\Users\DyllenOwens\Projects\omni\target\release\omni_overlay_dll.dll"
```

Expected console output:
```
INFO omni_host: Omni host starting — injecting overlay DLL pid=<PID> dll_path="..."
INFO omni_host::injector: Created remote thread — waiting for DLL to load
INFO omni_host::injector: DLL injection complete
INFO omni_host: DLL injection successful pid=<PID>
```

- [ ] **Step 4: Wait ~10 seconds then check the log**

```powershell
Get-Content $env:TEMP\omni_overlay.log
```

Expected output (timestamps will vary):
```
[1711555200] omni overlay DLL attached — spawning init thread
[1711555200] discovering swap chain vtable...
[1711555200] vtable discovery: Present=0x7ff..., ResizeBuffers=0x7ff...
[1711555200] installing Present hook...
[1711555200] installing ResizeBuffers hook...
[1711555200] all hooks installed successfully
[1711555201] Present hook active — frame 0
[1711555206] Present hook active — frame 300
[1711555211] Present hook active — frame 600
```

The frame counter messages confirm the hook is running every frame.

- [ ] **Step 5: Troubleshooting**

If the log shows "FATAL: hook installation failed":
- **D3D11CreateDeviceAndSwapChain failed**: The game may be DX12-only. Try a known DX11 title, or try injecting into a simple app like the DirectX SDK samples.
- **MinHook create_hook failed**: Another overlay (Discord, GeForce Experience, Steam) may have already hooked Present. Try disabling other overlays.
- If the DLL attaches but no vtable discovery message appears, the init thread may have crashed. Check Windows Event Viewer → Application logs for any crash reports.

- [ ] **Step 6: Test window resize**

If the game supports windowed mode, resize the window. Check the log for:
```
[...] ResizeBuffers called: 1280x720, buffers=2
```

- [ ] **Step 7: Commit any fixes discovered during testing**

```bash
git add -A
git commit -m "fix: address issues found during Present hook integration test"
```

---

## Phase 2 Complete — Summary

At this point you have:

1. `IDXGISwapChain::Present` hooked — our code runs every frame the game renders
2. `IDXGISwapChain::ResizeBuffers` hooked — ready to handle resolution changes in later phases
3. Frame counter confirming the hook is active and stable
4. Clean separation: `hook.rs` (discovery + installation), `present.rs` (hook implementations), `logging.rs` (shared logging)

**Next:** Phase 3 will use the hooked `Present` to acquire the game's `ID3D11Device` and `ID3D11DeviceContext`, create a render target view from the back buffer, and draw a simple colored rectangle as a proof-of-concept overlay.
