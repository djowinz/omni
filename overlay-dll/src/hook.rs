// Hook installation — Task 3.

use std::ffi::c_void;

use windows::Win32::Foundation::{BOOL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDeviceAndSwapChain, D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    IDXGISwapChain, DXGI_SWAP_CHAIN_DESC, DXGI_SWAP_EFFECT_DISCARD,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
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
//                           11=GetFullscreenState, 12=GetDesc, 13=ResizeBuffers, ...
pub const PRESENT_VTABLE_INDEX: usize = 8;
pub const RESIZE_BUFFERS_VTABLE_INDEX: usize = 13;

/// Raw vtable addresses captured from a temporary swap chain.
pub struct SwapChainVtable {
    pub present: *const c_void,
    pub resize_buffers: *const c_void,
}

// SAFETY: we only read these addresses (never dereference them as objects);
// they are valid for the lifetime of d3d11.dll.
unsafe impl Send for SwapChainVtable {}
unsafe impl Sync for SwapChainVtable {}

// ─── Dummy window helper ───────────────────────────────────────────────────────

/// Creates a minimal hidden window for use during vtable discovery.
/// The window is destroyed by the caller after the swap chain is released.
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

    // RegisterClassExW returns 0 on failure; ignore "already registered" scenario.
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

/// Minimal window procedure for the dummy window.
unsafe extern "system" fn dummy_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

// ─── Vtable discovery ─────────────────────────────────────────────────────────

/// Creates a temporary D3D11 device + swap chain, reads the vtable pointers for
/// `Present` and `ResizeBuffers`, then tears everything down.
pub unsafe fn discover_swapchain_vtable() -> Result<SwapChainVtable, String> {
    let hwnd = create_dummy_window()?;

    let swap_chain_desc = DXGI_SWAP_CHAIN_DESC {
        BufferDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_MODE_DESC {
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
        Windowed: BOOL(1),
        SwapEffect: DXGI_SWAP_EFFECT_DISCARD,
        Flags: 0,
    };

    let mut swap_chain: Option<IDXGISwapChain> = None;

    D3D11CreateDeviceAndSwapChain(
        None,                                  // pAdapter (use default)
        D3D_DRIVER_TYPE_HARDWARE,
        None,                                  // Software module
        D3D11_CREATE_DEVICE_FLAG(0),           // No debug flags
        None,                                  // pFeatureLevels (use defaults)
        D3D11_SDK_VERSION,
        Some(&swap_chain_desc),
        Some(&mut swap_chain),
        None,                                  // ppDevice (don't need it)
        None,                                  // pFeatureLevel out
        None,                                  // ppImmediateContext (don't need it)
    )
    .map_err(|e| format!("D3D11CreateDeviceAndSwapChain failed: {e}"))?;

    let swap_chain = swap_chain.ok_or_else(|| "swap chain pointer is null".to_owned())?;

    // The COM pointer's first field is a pointer to the vtable.
    // IDXGISwapChain is repr(transparent) over a raw pointer.
    let raw_ptr: *mut *const *const c_void = Interface::as_raw(&swap_chain) as _;
    let vtable: *const *const c_void = *raw_ptr;

    let present_fn       = *vtable.add(PRESENT_VTABLE_INDEX);
    let resize_buffers_fn = *vtable.add(RESIZE_BUFFERS_VTABLE_INDEX);

    crate::logging::log_to_file(&format!(
        "[hook] vtable discovered: Present={present_fn:p}, ResizeBuffers={resize_buffers_fn:p}"
    ));

    // Drop the swap chain COM object before destroying the window.
    drop(swap_chain);
    let _ = DestroyWindow(hwnd);

    Ok(SwapChainVtable {
        present: present_fn,
        resize_buffers: resize_buffers_fn,
    })
}

// ─── Hook installation ────────────────────────────────────────────────────────

/// Discovers the swap chain vtable, installs MinHook detours for `Present` and
/// `ResizeBuffers`, then enables all hooks.
pub unsafe fn install_hooks() -> Result<(), String> {
    let vtable = discover_swapchain_vtable()?;

    // Hook Present
    let original_present = minhook::MinHook::create_hook(
        vtable.present as *mut c_void,
        crate::present::hooked_present as *mut c_void,
    )
    .map_err(|s| format!("MinHook::create_hook(Present) failed: {s:?}"))?;

    crate::present::ORIGINAL_PRESENT =
        Some(std::mem::transmute::<*mut c_void, crate::present::PresentFn>(original_present));

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

    crate::logging::log_to_file("[hook] hooks installed and enabled");
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vtable_indices_are_correct() {
        assert_eq!(PRESENT_VTABLE_INDEX, 8);
        assert_eq!(RESIZE_BUFFERS_VTABLE_INDEX, 13);
    }
}
