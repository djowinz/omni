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
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISwapChain, IDXGISwapChain1,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_DISCARD,
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

// ─── Hook installation ────────────────────────────────────────────────────────

pub unsafe fn install_hooks() -> Result<(), String> {
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
    }
}
