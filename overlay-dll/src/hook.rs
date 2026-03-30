// Hook installation — vtable discovery via DXGI 1.2 path (CreateSwapChainForHwnd)
// and fallback to legacy D3D11CreateDeviceAndSwapChain.

use std::ffi::c_void;

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, ID3D11Device,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Direct3D12::ID3D12CommandQueue;
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISwapChain, IDXGISwapChain1,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    DXGI_SWAP_EFFECT_FLIP_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, DXGI_SCALING_STRETCH,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetModuleHandleA};
use windows::core::s;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassExW, HMENU, WNDCLASSEXW,
    WS_OVERLAPPEDWINDOW, WINDOW_EX_STYLE,
};
use windows::core::{w, Interface, PCWSTR};

// ─── Vtable indices ────────────────────────────────────────────────────────────
// IDXGISwapChain vtable layout (0-based):
//   IUnknown_Vtbl         : 0=QueryInterface, 1=AddRef, 2=Release
//   IDXGIObject_Vtbl      : 3=SetPrivateData, 4=SetPrivateDataInterface, 5=GetPrivateData, 6=GetParent
//   IDXGIDeviceSubObject  : 7=GetDevice
//   IDXGISwapChain        : 8=Present, 9=GetBuffer, 10=SetFullscreenState,
//                           11=GetFullscreenState, 12=GetDesc, 13=ResizeBuffers,
//                           14=ResizeTarget, 15=GetContainingOutput,
//                           16=GetFrameStatistics, 17=GetLastPresentCount
//   IDXGISwapChain1       : 18=GetDesc1, 19=GetFullscreenDesc, 20=GetHwnd,
//                           21=GetCoreWindow, 22=Present1, ...
pub const PRESENT_VTABLE_INDEX: usize = 8;
pub const PRESENT1_VTABLE_INDEX: usize = 22;
pub const RESIZE_BUFFERS_VTABLE_INDEX: usize = 13;

// IDXGIFactory2 vtable layout (0-based):
//   IUnknown          : 0=QueryInterface, 1=AddRef, 2=Release
//   IDXGIObject       : 3=SetPrivateData, 4=SetPrivateDataInterface, 5=GetPrivateData, 6=GetParent
//   IDXGIFactory      : 7=EnumAdapters, 8=MakeWindowAssociation, 9=GetWindowAssociation,
//                        10=CreateSwapChain, 11=CreateSoftwareAdapter
//   IDXGIFactory1     : 12=EnumAdapters1, 13=IsCurrent
//   IDXGIFactory2     : 14=IsWindowedStereoEnabled, 15=CreateSwapChainForHwnd, ...
pub const CREATE_SWAP_CHAIN_FOR_HWND_VTABLE_INDEX: usize = 15;

// ID3D12CommandQueue vtable layout (0-based):
//   IUnknown           : 0=QueryInterface, 1=AddRef, 2=Release
//   ID3D12Object       : 3=GetPrivateData, 4=SetPrivateData, 5=SetPrivateDataInterface, 6=SetName
//   ID3D12DeviceChild  : 7=GetDevice
//   ID3D12Pageable     : (no own methods)
//   ID3D12CommandQueue : 8=UpdateTileMappings, 9=CopyTileMappings, 10=ExecuteCommandLists, ...
pub const EXECUTE_COMMAND_LISTS_VTABLE_INDEX: usize = 10;

/// Captured DX12 command queue (if the game is using DX12).
/// Set by the CreateSwapChainForHwnd hook or the ExecuteCommandLists hook.
pub static mut CAPTURED_COMMAND_QUEUE: Option<ID3D12CommandQueue> = None;

/// Type alias for the original CreateSwapChainForHwnd function pointer.
pub type CreateSwapChainForHwndFn = unsafe extern "system" fn(
    *mut c_void,    // this (IDXGIFactory2)
    *mut c_void,    // pDevice (IUnknown — for DX12 this is ID3D12CommandQueue)
    HWND,           // hWnd
    *const DXGI_SWAP_CHAIN_DESC1,
    *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    *mut c_void,    // pRestrictToOutput (IDXGIOutput)
    *mut *mut c_void, // ppSwapChain (IDXGISwapChain1**)
) -> windows::core::HRESULT;

pub static mut ORIGINAL_CREATE_SWAP_CHAIN_FOR_HWND: Option<CreateSwapChainForHwndFn> = None;

/// Type alias for the original ExecuteCommandLists function pointer.
pub type ExecuteCommandListsFn = unsafe extern "system" fn(
    *mut c_void,    // this (ID3D12CommandQueue)
    u32,            // NumCommandLists
    *const *mut c_void, // ppCommandLists
);

pub static mut ORIGINAL_EXECUTE_COMMAND_LISTS: Option<ExecuteCommandListsFn> = None;

/// Raw vtable addresses captured from a temporary swap chain.
pub struct SwapChainVtable {
    pub present: *const c_void,
    pub present1: *const c_void,
    pub resize_buffers: *const c_void,
}

// SAFETY: we only read these addresses (never dereference them as objects);
// they are valid for the lifetime of dxgi.dll.
unsafe impl Send for SwapChainVtable {}
unsafe impl Sync for SwapChainVtable {}

// ─── Dummy window helper ───────────────────────────────────────────────────────

unsafe fn create_dummy_window() -> Result<HWND, String> {
    let hinstance: HINSTANCE = GetModuleHandleW(PCWSTR::null())
        .map_err(|e| format!("GetModuleHandleW failed: {e}"))?
        .into();

    let class_name = w!("OmniDummyWndClass");

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(dummy_wnd_proc),
        hInstance: hinstance,
        lpszClassName: class_name,
        ..Default::default()
    };

    RegisterClassExW(&wc);

    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        class_name,
        w!("OmniDummy"),
        WS_OVERLAPPEDWINDOW,
        0, 0, 1, 1,
        HWND::default(),
        HMENU::default(),
        hinstance,
        None,
    )
    .map_err(|e| format!("CreateWindowExW failed: {e}"))?;

    Ok(hwnd)
}

unsafe extern "system" fn dummy_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

// ─── Vtable reading helper ──────────────────────────────────────────────────────

/// Read Present, Present1, and ResizeBuffers function pointers from an IDXGISwapChain1.
/// We read from IDXGISwapChain1 (not base IDXGISwapChain) so we can also get Present1.
unsafe fn read_vtable(swap_chain: &IDXGISwapChain1) -> SwapChainVtable {
    let raw_ptr: *mut *const *const c_void = Interface::as_raw(swap_chain) as _;
    let vtable: *const *const c_void = *raw_ptr;

    SwapChainVtable {
        present: *vtable.add(PRESENT_VTABLE_INDEX),
        present1: *vtable.add(PRESENT1_VTABLE_INDEX),
        resize_buffers: *vtable.add(RESIZE_BUFFERS_VTABLE_INDEX),
    }
}

// ─── Vtable discovery ─────────────────────────────────────────────────────────

/// Try the DXGI 1.2 path first (CreateSwapChainForHwnd with flip model),
/// which is what modern games and SDL use. Falls back to the legacy
/// D3D11CreateDeviceAndSwapChain path.
pub unsafe fn discover_swapchain_vtable() -> Result<SwapChainVtable, String> {
    let hwnd = create_dummy_window()?;

    // Create D3D11 device (no swap chain yet)
    let mut device: Option<ID3D11Device> = None;
    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        None,
        D3D11_CREATE_DEVICE_FLAG(0),
        None,
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        None,
    )
    .map_err(|e| format!("D3D11CreateDevice failed: {e}"))?;

    let device = device.ok_or("D3D11CreateDevice returned null device")?;

    // Get DXGI factory via device → IDXGIDevice → adapter → parent factory
    let result = discover_via_factory2(&device, hwnd);

    let vtable = match result {
        Ok(vt) => {
            crate::logging::log_to_file("[hook] vtable discovered via DXGI 1.2 (CreateSwapChainForHwnd)");
            vt
        }
        Err(e) => {
            crate::logging::log_to_file(&format!(
                "[hook] DXGI 1.2 path failed ({e}), falling back to legacy path"
            ));
            discover_via_legacy(hwnd)?
        }
    };

    crate::logging::log_to_file(&format!(
        "[hook] vtable: Present={:p}, Present1={:p}, ResizeBuffers={:p}",
        vtable.present, vtable.present1, vtable.resize_buffers
    ));

    drop(device);
    let _ = DestroyWindow(hwnd);

    Ok(vtable)
}

/// DXGI 1.2 path: CreateSwapChainForHwnd with flip model.
unsafe fn discover_via_factory2(device: &ID3D11Device, hwnd: HWND) -> Result<SwapChainVtable, String> {
    let dxgi_device: IDXGIDevice = device
        .cast()
        .map_err(|e| format!("device.cast::<IDXGIDevice>(): {e}"))?;

    let adapter = dxgi_device
        .GetAdapter()
        .map_err(|e| format!("GetAdapter: {e}"))?;

    let factory: IDXGIFactory2 = adapter
        .GetParent()
        .map_err(|e| format!("GetParent::<IDXGIFactory2>(): {e}"))?;

    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: 2,
        Height: 2,
        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2, // flip model requires ≥ 2
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_DISCARD,
        Scaling: DXGI_SCALING_STRETCH,
        ..Default::default()
    };

    let swap_chain1: IDXGISwapChain1 = factory
        .CreateSwapChainForHwnd(device, hwnd, &desc, None, None)
        .map_err(|e| format!("CreateSwapChainForHwnd: {e}"))?;

    let vtable = read_vtable(&swap_chain1);

    drop(swap_chain1);

    Ok(vtable)
}

/// Legacy fallback: D3D11CreateDeviceAndSwapChain, then QueryInterface for IDXGISwapChain1.
unsafe fn discover_via_legacy(hwnd: HWND) -> Result<SwapChainVtable, String> {
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::Graphics::Direct3D11::D3D11CreateDeviceAndSwapChain;
    use windows::Win32::Graphics::Dxgi::{DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_DISCARD};
    use windows::Win32::Graphics::Dxgi::Common::DXGI_MODE_DESC;

    let desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: DXGI_MODE_DESC {
            Width: 2,
            Height: 2,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            ..Default::default()
        },
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 1,
        OutputWindow: hwnd,
        Windowed: BOOL(1),
        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
        Flags: 0,
    };

    let mut swap_chain: Option<IDXGISwapChain> = None;

    D3D11CreateDeviceAndSwapChain(
        None, D3D_DRIVER_TYPE_HARDWARE, None,
        D3D11_CREATE_DEVICE_FLAG(0), None, D3D11_SDK_VERSION,
        Some(&desc), Some(&mut swap_chain),
        None, None, None,
    )
    .map_err(|e| format!("D3D11CreateDeviceAndSwapChain failed: {e}"))?;

    let swap_chain = swap_chain.ok_or("legacy swap chain is null")?;

    // Cast up to IDXGISwapChain1 to also get Present1
    let swap_chain1: IDXGISwapChain1 = swap_chain
        .cast()
        .map_err(|e| format!("cast to IDXGISwapChain1: {e}"))?;

    let vtable = read_vtable(&swap_chain1);

    drop(swap_chain1);
    Ok(vtable)
}

// ─── Diagnostics ──────────────────────────────────────────────────────────────

/// Check which graphics-related DLLs the game process has loaded.
/// This tells us whether the game is actually using DX11, DX12, OpenGL, or Vulkan.
unsafe fn log_loaded_graphics_modules() {
    let mut found = Vec::new();

    if GetModuleHandleA(s!("d3d11.dll")).is_ok()    { found.push("Direct3D 11"); }
    if GetModuleHandleA(s!("d3d12.dll")).is_ok()    { found.push("Direct3D 12"); }
    if GetModuleHandleA(s!("dxgi.dll")).is_ok()      { found.push("DXGI"); }
    if GetModuleHandleA(s!("opengl32.dll")).is_ok()  { found.push("OpenGL"); }
    if GetModuleHandleA(s!("vulkan-1.dll")).is_ok()  { found.push("Vulkan"); }
    if GetModuleHandleA(s!("d3d9.dll")).is_ok()      { found.push("Direct3D 9"); }

    crate::logging::log_to_file(&format!(
        "[hook] loaded graphics modules: {}",
        if found.is_empty() { "NONE".to_string() } else { found.join(", ") }
    ));
}

// ─── CreateSwapChainForHwnd hook ─────────────────────────────────────────────

pub unsafe extern "system" fn hooked_create_swap_chain_for_hwnd(
    this: *mut c_void,
    p_device: *mut c_void,
    hwnd: HWND,
    p_desc: *const DXGI_SWAP_CHAIN_DESC1,
    p_fullscreen_desc: *const DXGI_SWAP_CHAIN_FULLSCREEN_DESC,
    p_restrict_to_output: *mut c_void,
    pp_swap_chain: *mut *mut c_void,
) -> windows::core::HRESULT {
    // Try to QueryInterface pDevice for ID3D12CommandQueue.
    // For DX12 games, pDevice is actually the command queue.
    // For DX11 games, this will fail — that's fine.
    if CAPTURED_COMMAND_QUEUE.is_none() && !p_device.is_null() {
        let unknown: &windows::core::IUnknown = std::mem::transmute(&p_device);
        match unknown.cast::<ID3D12CommandQueue>() {
            Ok(queue) => {
                crate::logging::log_to_file(
                    "[hook] captured ID3D12CommandQueue from CreateSwapChainForHwnd",
                );
                CAPTURED_COMMAND_QUEUE = Some(queue);
            }
            Err(_) => {
                crate::logging::log_to_file(
                    "[hook] pDevice is not ID3D12CommandQueue (DX11 game)",
                );
            }
        }
    }

    if let Some(original) = ORIGINAL_CREATE_SWAP_CHAIN_FOR_HWND {
        original(this, p_device, hwnd, p_desc, p_fullscreen_desc, p_restrict_to_output, pp_swap_chain)
    } else {
        windows::core::HRESULT(-1)
    }
}

// ─── ExecuteCommandLists hook ────────────────────────────────────────────────

/// Hooks ID3D12CommandQueue::ExecuteCommandLists to capture the game's DIRECT command queue.
/// This fires every frame the game submits rendering work, so it captures the queue
/// immediately — even after re-injection when CreateSwapChainForHwnd doesn't fire.
/// Only captures DIRECT queues — D3D11On12 requires a direct queue, not compute/copy.
unsafe extern "system" fn hooked_execute_command_lists(
    this: *mut c_void,
    num_command_lists: u32,
    pp_command_lists: *const *mut c_void,
) {
    use windows::Win32::Graphics::Direct3D12::D3D12_COMMAND_LIST_TYPE_DIRECT;

    // Capture a DIRECT queue on first call
    if CAPTURED_COMMAND_QUEUE.is_none() && !this.is_null() {
        let unknown: &windows::core::IUnknown = std::mem::transmute(&this);
        if let Ok(queue) = unknown.cast::<ID3D12CommandQueue>() {
            let desc = queue.GetDesc();
            if desc.Type == D3D12_COMMAND_LIST_TYPE_DIRECT {
                crate::logging::log_to_file(
                    "[hook] captured DIRECT ID3D12CommandQueue from ExecuteCommandLists",
                );
                CAPTURED_COMMAND_QUEUE = Some(queue);
            } else {
                crate::logging::log_to_file(&format!(
                    "[hook] skipping non-DIRECT command queue (type={:?}) from ExecuteCommandLists",
                    desc.Type
                ));
            }
        }
    }

    if let Some(original) = ORIGINAL_EXECUTE_COMMAND_LISTS {
        original(this, num_command_lists, pp_command_lists);
    }
}

// ─── Factory2 vtable discovery ──────────────────────────────────────────────

/// Discover the CreateSwapChainForHwnd vtable address from IDXGIFactory2.
/// Creates a temporary D3D11 device, walks DXGI device → adapter → factory,
/// then reads the vtable pointer at the correct index.
unsafe fn discover_factory2_vtable() -> Result<*const c_void, String> {
    // Create a temporary D3D11 device
    let mut device: Option<ID3D11Device> = None;
    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        None,
        D3D11_CREATE_DEVICE_FLAG(0),
        None,
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        None,
    )
    .map_err(|e| format!("D3D11CreateDevice (factory2 discovery) failed: {e}"))?;

    let device = device.ok_or("D3D11CreateDevice returned null device (factory2 discovery)")?;

    let dxgi_device: IDXGIDevice = device
        .cast()
        .map_err(|e| format!("cast to IDXGIDevice (factory2): {e}"))?;

    let adapter = dxgi_device
        .GetAdapter()
        .map_err(|e| format!("GetAdapter (factory2): {e}"))?;

    let factory: IDXGIFactory2 = adapter
        .GetParent()
        .map_err(|e| format!("GetParent::<IDXGIFactory2> (factory2): {e}"))?;

    // Read the vtable
    let raw_ptr: *mut *const *const c_void = Interface::as_raw(&factory) as _;
    let vtable: *const *const c_void = *raw_ptr;
    let create_swap_chain_for_hwnd = *vtable.add(CREATE_SWAP_CHAIN_FOR_HWND_VTABLE_INDEX);

    crate::logging::log_to_file(&format!(
        "[hook] factory2 vtable: CreateSwapChainForHwnd={:p}",
        create_swap_chain_for_hwnd
    ));

    Ok(create_swap_chain_for_hwnd)
}

// ─── Deferred DX12 hook setup ───────────────────────────────────────────────

/// Hook ExecuteCommandLists using an existing D3D12 device (the game's device).
/// Called lazily from the renderer when DX12 is first detected, avoiding the need
/// to call D3D12CreateDevice ourselves (which races with the game's init).
pub unsafe fn hook_execute_command_lists_deferred(
    device: &windows::Win32::Graphics::Direct3D12::ID3D12Device,
) -> Result<(), String> {
    use windows::Win32::Graphics::Direct3D12::{
        D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC,
    };

    // Already hooked?
    if ORIGINAL_EXECUTE_COMMAND_LISTS.is_some() {
        return Ok(());
    }

    // Create a temporary queue from the game's device to read the vtable
    let desc = D3D12_COMMAND_QUEUE_DESC {
        Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
        ..Default::default()
    };

    let queue: ID3D12CommandQueue = device
        .CreateCommandQueue(&desc)
        .map_err(|e| format!("CreateCommandQueue (vtable discovery) failed: {e}"))?;

    let raw_ptr: *mut *const *const c_void = Interface::as_raw(&queue) as _;
    let vtable: *const *const c_void = *raw_ptr;
    let execute_cl_addr = *vtable.add(EXECUTE_COMMAND_LISTS_VTABLE_INDEX);

    crate::logging::log_to_file(&format!(
        "[hook] ID3D12CommandQueue::ExecuteCommandLists at {:p}",
        execute_cl_addr,
    ));

    // Drop the temp queue before hooking
    drop(queue);

    let original_ecl = minhook::MinHook::create_hook(
        execute_cl_addr as *mut c_void,
        hooked_execute_command_lists as *mut c_void,
    )
    .map_err(|s| format!("MinHook::create_hook(ExecuteCommandLists) failed: {s:?}"))?;

    ORIGINAL_EXECUTE_COMMAND_LISTS = Some(std::mem::transmute::<
        *mut c_void,
        ExecuteCommandListsFn,
    >(original_ecl));

    minhook::MinHook::enable_all_hooks()
        .map_err(|s| format!("MinHook::enable_all_hooks (ECL) failed: {s:?}"))?;

    crate::logging::log_to_file("[hook] ExecuteCommandLists hook created and enabled (deferred)");

    Ok(())
}

// ─── Hook state ──────────────────────────────────────────────────────────────

use std::sync::atomic::{AtomicBool, Ordering};

/// Guard flag — true once hooks have been installed. Prevents double-hooking
/// if the DLL is loaded/initialized more than once in the same process.
pub static HOOKS_INSTALLED: AtomicBool = AtomicBool::new(false);

// ─── Hook installation ────────────────────────────────────────────────────────

pub unsafe fn install_hooks() -> Result<(), String> {
    // Prevent double-initialization.
    if HOOKS_INSTALLED.swap(true, Ordering::SeqCst) {
        crate::logging::log_to_file("[hook] hooks already installed — skipping");
        return Ok(());
    }

    // Diagnostic: check which graphics DLLs the game has loaded
    log_loaded_graphics_modules();

    let vtable = discover_swapchain_vtable()?;

    // Hook Present
    let original_present = minhook::MinHook::create_hook(
        vtable.present as *mut c_void,
        crate::present::hooked_present as *mut c_void,
    )
    .map_err(|s| format!("MinHook::create_hook(Present) failed: {s:?}"))?;

    crate::present::ORIGINAL_PRESENT =
        Some(std::mem::transmute::<*mut c_void, crate::present::PresentFn>(original_present));

    // Hook Present1 (IDXGISwapChain1::Present1 — used by SDL and modern games)
    let original_present1 = minhook::MinHook::create_hook(
        vtable.present1 as *mut c_void,
        crate::present::hooked_present1 as *mut c_void,
    )
    .map_err(|s| format!("MinHook::create_hook(Present1) failed: {s:?}"))?;

    crate::present::ORIGINAL_PRESENT1 =
        Some(std::mem::transmute::<*mut c_void, crate::present::Present1Fn>(original_present1));

    // Hook ResizeBuffers
    let original_resize = minhook::MinHook::create_hook(
        vtable.resize_buffers as *mut c_void,
        crate::present::hooked_resize_buffers as *mut c_void,
    )
    .map_err(|s| format!("MinHook::create_hook(ResizeBuffers) failed: {s:?}"))?;

    crate::present::ORIGINAL_RESIZE_BUFFERS =
        Some(std::mem::transmute::<*mut c_void, crate::present::ResizeBuffersFn>(original_resize));

    // Hook CreateSwapChainForHwnd (captures DX12 command queue)
    match discover_factory2_vtable() {
        Ok(create_scfh_addr) => {
            let original_create_scfh = minhook::MinHook::create_hook(
                create_scfh_addr as *mut c_void,
                hooked_create_swap_chain_for_hwnd as *mut c_void,
            )
            .map_err(|s| format!("MinHook::create_hook(CreateSwapChainForHwnd) failed: {s:?}"))?;

            ORIGINAL_CREATE_SWAP_CHAIN_FOR_HWND = Some(std::mem::transmute::<
                *mut c_void,
                CreateSwapChainForHwndFn,
            >(original_create_scfh));
        }
        Err(e) => {
            crate::logging::log_to_file(&format!(
                "[hook] WARNING: factory2 vtable discovery failed ({e}), CreateSwapChainForHwnd hook skipped"
            ));
        }
    }

    // NOTE: ExecuteCommandLists hook is NOT installed here — it's deferred
    // to when the renderer first detects DX12 (via hook_execute_command_lists_deferred).
    // This avoids calling D3D12CreateDevice during game startup, which races with
    // the game's own D3D12 initialization and can cause crashes.

    minhook::MinHook::enable_all_hooks()
        .map_err(|s| format!("MinHook::enable_all_hooks failed: {s:?}"))?;

    crate::logging::log_to_file("[hook] all hooks installed and enabled");

    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtable_indices_are_correct() {
        assert_eq!(PRESENT_VTABLE_INDEX, 8);
        assert_eq!(PRESENT1_VTABLE_INDEX, 22);
        assert_eq!(RESIZE_BUFFERS_VTABLE_INDEX, 13);
        assert_eq!(CREATE_SWAP_CHAIN_FOR_HWND_VTABLE_INDEX, 15);
        assert_eq!(EXECUTE_COMMAND_LISTS_VTABLE_INDEX, 10);
    }
}
