#![windows_subsystem = "windows"]
//! External overlay process for anti-cheat protected games.
//!
//! Creates a transparent, topmost, click-through window that tracks a target
//! game window and renders a BGRA bitmap read from shared memory using D3D11
//! via DirectComposition (per-pixel alpha transparency).
//!
//! Usage: `omni-overlay.exe --hwnd <window_handle>`

use std::sync::atomic::Ordering;
use std::time::Duration;

use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use omni_shared::{BitmapHeader, BITMAP_IPC_VERSION, BITMAP_SHM_NAME, PIXEL_DATA_OFFSET};
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11_SUBRESOURCE_DATA,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_UNKNOWN,
    DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISwapChain1, DXGI_PRESENT, DXGI_SWAP_CHAIN_DESC1,
    DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Memory::{MapViewOfFile, OpenFileMappingW, FILE_MAP_READ};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetClientRect, GetForegroundWindow,
    IsIconic, IsWindow, PeekMessageW, RegisterClassExW, SetWindowPos, ShowWindow, TranslateMessage,
    HWND_TOPMOST, MSG, PM_REMOVE, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WM_QUIT, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

// ---------------------------------------------------------------------------
// CLI parsing
// ---------------------------------------------------------------------------

fn parse_args() -> HWND {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--hwnd" && i + 1 < args.len() {
            let val = &args[i + 1];
            let raw: isize = val.parse().unwrap_or_else(|_| {
                error!(value = %val, "invalid --hwnd value");
                std::process::exit(1);
            });
            return HWND(raw as *mut _);
        }
        i += 1;
    }
    error!("usage: omni-overlay.exe --hwnd <window_handle>");
    std::process::exit(1);
}

// ---------------------------------------------------------------------------
// Window creation
// ---------------------------------------------------------------------------

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn create_overlay_window() -> windows::core::Result<HWND> {
    unsafe {
        let class_name = w!("OmniOverlayClass");
        let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None)?;

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        RegisterClassExW(&wc);

        let ex_style =
            WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;

        let hwnd = CreateWindowExW(
            ex_style,
            class_name,
            w!("Omni Overlay"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            HWND::default(),
            None,
            hinstance,
            None,
        )?;

        Ok(hwnd)
    }
}

// ---------------------------------------------------------------------------
// Graphics setup: D3D11 + DXGI swap chain + DirectComposition
// ---------------------------------------------------------------------------

struct GraphicsState {
    _dcomp_device: IDCompositionDevice,
    _dcomp_target: IDCompositionTarget,
    _dcomp_visual: IDCompositionVisual,
    swap_chain: IDXGISwapChain1,
    d3d11_device: ID3D11Device,
    d3d11_context: ID3D11DeviceContext,
    current_width: u32,
    current_height: u32,
    /// Staging texture for bitmap upload — created lazily when dimensions are known.
    staging_texture: Option<ID3D11Texture2D>,
    staging_width: u32,
    staging_height: u32,
}

impl GraphicsState {
    unsafe fn init(overlay_hwnd: HWND) -> windows::core::Result<Self> {
        // 1. Create D3D11 device
        let mut d3d11_device = None;
        let mut d3d11_context = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut d3d11_device),
            None,
            Some(&mut d3d11_context),
        )?;
        let d3d11_device = d3d11_device.unwrap();
        let d3d11_context = d3d11_context.unwrap();

        // 2. Get DXGI factory
        let dxgi_device: IDXGIDevice = d3d11_device.cast()?;
        let dxgi_adapter = dxgi_device.GetAdapter()?;
        let dxgi_factory: IDXGIFactory2 = dxgi_adapter.GetParent()?;

        // 3. Create swap chain for composition (enables per-pixel alpha)
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: 1,
            Height: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: false.into(),
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: windows::Win32::Graphics::Dxgi::DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: 0,
        };

        let swap_chain = dxgi_factory.CreateSwapChainForComposition(&d3d11_device, &desc, None)?;

        // 4. DirectComposition: bind swap chain to overlay window
        let dcomp_device: IDCompositionDevice = DCompositionCreateDevice(None)?;
        let dcomp_target = dcomp_device.CreateTargetForHwnd(overlay_hwnd, true)?;
        let dcomp_visual = dcomp_device.CreateVisual()?;
        dcomp_visual.SetContent(&swap_chain)?;
        dcomp_target.SetRoot(&dcomp_visual)?;
        dcomp_device.Commit()?;

        Ok(Self {
            _dcomp_device: dcomp_device,
            _dcomp_target: dcomp_target,
            _dcomp_visual: dcomp_visual,
            swap_chain,
            d3d11_device,
            d3d11_context,
            current_width: 1,
            current_height: 1,
            staging_texture: None,
            staging_width: 0,
            staging_height: 0,
        })
    }

    /// Resize the swap chain.
    unsafe fn resize(&mut self, width: u32, height: u32) -> windows::core::Result<()> {
        if width == 0 || height == 0 {
            return Ok(());
        }

        self.swap_chain.ResizeBuffers(
            0,
            width,
            height,
            DXGI_FORMAT_UNKNOWN,
            DXGI_SWAP_CHAIN_FLAG(0),
        )?;
        self.current_width = width;
        self.current_height = height;

        Ok(())
    }

    /// Ensure the staging texture matches the given dimensions.
    unsafe fn ensure_staging_texture(
        &mut self,
        width: u32,
        height: u32,
    ) -> windows::core::Result<()> {
        if self.staging_texture.is_some()
            && self.staging_width == width
            && self.staging_height == height
        {
            return Ok(());
        }

        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: 0,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };

        // Create with zeroed initial data so the texture starts transparent
        let zero_data = vec![0u8; (width * height * 4) as usize];
        let init_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: zero_data.as_ptr() as *const _,
            SysMemPitch: width * 4,
            SysMemSlicePitch: 0,
        };

        let mut texture = None;
        self.d3d11_device
            .CreateTexture2D(&desc, Some(&init_data), Some(&mut texture))?;
        self.staging_texture = texture;
        self.staging_width = width;
        self.staging_height = height;

        info!(width, height, "staging texture created");

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Bitmap shared memory reader
// ---------------------------------------------------------------------------

struct BitmapReader {
    ptr: *const u8,
    _handle: windows::Win32::Foundation::HANDLE,
}

impl BitmapReader {
    fn open() -> Option<Self> {
        let name_wide: Vec<u16> = BITMAP_SHM_NAME
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let handle =
            unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, PCWSTR(name_wide.as_ptr())) }.ok()?;

        let ptr = unsafe { MapViewOfFile(handle, FILE_MAP_READ, 0, 0, 0) };
        if ptr.Value.is_null() {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
            return None;
        }

        let base = ptr.Value as *const u8;

        // Check protocol version
        let header = unsafe { &*(base as *const BitmapHeader) };
        if header.version != BITMAP_IPC_VERSION {
            error!(
                expected = BITMAP_IPC_VERSION,
                found = header.version,
                "bitmap IPC version mismatch"
            );
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
            }
            return None;
        }

        Some(Self {
            ptr: base,
            _handle: handle,
        })
    }

    fn header(&self) -> &BitmapHeader {
        unsafe { &*(self.ptr as *const BitmapHeader) }
    }

    fn pixel_data(&self) -> *const u8 {
        unsafe { self.ptr.add(PIXEL_DATA_OFFSET) }
    }
}

impl Drop for BitmapReader {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::System::Memory::UnmapViewOfFile(
                windows::Win32::System::Memory::MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.ptr as *mut std::ffi::c_void,
                },
            );
            let _ = windows::Win32::Foundation::CloseHandle(self._handle);
        }
    }
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let target_hwnd = parse_args();

    // Verify target window exists
    if !unsafe { IsWindow(target_hwnd) }.as_bool() {
        error!(hwnd = ?target_hwnd, "target window handle is not a valid window");
        std::process::exit(1);
    }

    let overlay_hwnd = create_overlay_window().unwrap_or_else(|e| {
        error!(err = %e, "failed to create overlay window");
        std::process::exit(1);
    });

    let mut gfx = unsafe { GraphicsState::init(overlay_hwnd) }.unwrap_or_else(|e| {
        error!(err = %e, "failed to initialize graphics");
        std::process::exit(1);
    });

    let mut shm: Option<BitmapReader> = None;
    let mut last_rect = RECT::default();
    let mut overlay_visible = false;
    let mut resized_this_frame;
    let mut last_sequence: u64 = 0;

    info!(hwnd = ?target_hwnd, "entering render loop");

    loop {
        // Frame pacing is provided by Present(1, ...) VSync at the end of each
        // iteration. No explicit sleep needed — VSync blocks until the next
        // display refresh, naturally capping to the monitor's refresh rate.

        // Process window messages first (non-blocking)
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE).as_bool() {
                if msg.message == WM_QUIT {
                    info!("WM_QUIT received, exiting");
                    return;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        // Check if target window still exists
        if !unsafe { IsWindow(target_hwnd) }.as_bool() {
            info!(hwnd = ?target_hwnd, "target window closed, exiting");
            break;
        }

        // Check if game is minimized
        if unsafe { IsIconic(target_hwnd) }.as_bool() {
            if overlay_visible {
                unsafe {
                    let _ = ShowWindow(overlay_hwnd, SW_HIDE);
                }
                overlay_visible = false;
            }
            continue;
        }

        // Track game window's client area position/size.
        // Use GetClientRect for dimensions (excludes title bar/borders)
        // and ClientToScreen for screen position of the client area origin.
        resized_this_frame = false;
        let mut client_rect = RECT::default();
        let mut game_rect = RECT::default();
        let got_client = unsafe { GetClientRect(target_hwnd, &mut client_rect) }.is_ok();
        if got_client {
            // Convert client area origin to screen coordinates
            let mut pt = windows::Win32::Foundation::POINT { x: 0, y: 0 };
            let _ = unsafe { ClientToScreen(target_hwnd, &mut pt) };
            game_rect = RECT {
                left: pt.x,
                top: pt.y,
                right: pt.x + client_rect.right,
                bottom: pt.y + client_rect.bottom,
            };
        }
        if got_client && game_rect != last_rect {
            let w = (game_rect.right - game_rect.left).max(1) as u32;
            let h = (game_rect.bottom - game_rect.top).max(1) as u32;

            unsafe {
                if SetWindowPos(
                    overlay_hwnd,
                    HWND_TOPMOST,
                    game_rect.left,
                    game_rect.top,
                    w as i32,
                    h as i32,
                    SWP_NOACTIVATE,
                )
                .is_err()
                {
                    warn!("SetWindowPos failed");
                }
            }

            // Resize swap chain if dimensions changed
            if w != gfx.current_width || h != gfx.current_height {
                if let Err(e) = unsafe { gfx.resize(w, h) } {
                    error!(err = %e, width = w, height = h, "swap chain resize failed");
                }
                // Skip rendering this frame — let the swap chain settle
                resized_this_frame = true;
            }

            last_rect = game_rect;
        }

        // Show overlay as TOPMOST only when the game is the foreground window.
        // When the game loses focus, demote to non-topmost and hide.
        // This is the standard approach used by Discord, Steam, etc.
        // Show overlay only when the game is the foreground window.
        // The window is created with WS_EX_TOPMOST so it stays above the
        // game when visible. We simply hide/show based on foreground state.
        // This is the same approach Discord uses for its overlay.
        let fg = unsafe { GetForegroundWindow() };
        let game_is_foreground = fg == target_hwnd;

        if game_is_foreground && !overlay_visible {
            unsafe {
                let _ = ShowWindow(overlay_hwnd, SW_SHOWNOACTIVATE);
            }
            overlay_visible = true;
        } else if !game_is_foreground && overlay_visible {
            unsafe {
                let _ = ShowWindow(overlay_hwnd, SW_HIDE);
            }
            overlay_visible = false;
        }

        // Skip all rendering when overlay is hidden
        if !overlay_visible {
            std::thread::sleep(Duration::from_millis(16));
            continue;
        }

        // Skip rendering on resize frames to avoid artifacts
        if resized_this_frame {
            continue;
        }

        // Try to open shared memory if not connected
        if shm.is_none() {
            shm = BitmapReader::open();
            if shm.is_some() {
                info!(name = BITMAP_SHM_NAME, "shared memory opened");
            }
        }

        // Read bitmap from shared memory and blit to swap chain
        unsafe {
            let has_content = if let Some(reader) = &shm {
                let header = reader.header();

                // Check visibility flag
                if !header.is_visible() {
                    if overlay_visible {
                        let _ = ShowWindow(overlay_hwnd, SW_HIDE);
                        overlay_visible = false;
                    }
                    // Still present a cleared frame to avoid stale content
                    false
                } else {
                    if !overlay_visible {
                        let _ = ShowWindow(overlay_hwnd, SW_SHOWNOACTIVATE);
                        overlay_visible = true;
                    }

                    let seq = header.write_sequence.load(Ordering::Acquire);
                    let bw = header.width;
                    let bh = header.height;

                    if seq > 0 && bw > 0 && bh > 0 {
                        // Ensure staging texture exists at correct size
                        if let Err(e) = gfx.ensure_staging_texture(bw, bh) {
                            error!(err = %e, width = bw, height = bh, "failed to create staging texture");
                            false
                        } else if let Some(staging) = &gfx.staging_texture {
                            // Upload dirty region (or full frame if sequence jumped)
                            if seq != last_sequence {
                                let row_bytes = header.row_bytes;
                                let pixels = reader.pixel_data();

                                // Determine the region to update
                                let (dx, dy, dw, dh) =
                                    if last_sequence == 0 || seq != last_sequence + 1 {
                                        // Full frame update
                                        (0, 0, bw, bh)
                                    } else {
                                        // Dirty region from header
                                        let dx = header.dirty_x.min(bw);
                                        let dy = header.dirty_y.min(bh);
                                        let dw = header.dirty_w.min(bw - dx);
                                        let dh = header.dirty_h.min(bh - dy);
                                        if dw == 0 || dh == 0 {
                                            (0, 0, bw, bh) // fallback to full
                                        } else {
                                            (dx, dy, dw, dh)
                                        }
                                    };

                                last_sequence = seq;

                                // UpdateSubresource with the dirty region
                                let src_offset =
                                    (dy as usize) * (row_bytes as usize) + (dx as usize) * 4;
                                let src_ptr = pixels.add(src_offset);

                                let dst_box = D3D11_BOX {
                                    left: dx,
                                    top: dy,
                                    front: 0,
                                    right: dx + dw,
                                    bottom: dy + dh,
                                    back: 1,
                                };

                                gfx.d3d11_context.UpdateSubresource(
                                    staging,
                                    0,
                                    Some(&dst_box),
                                    src_ptr as *const _,
                                    row_bytes,
                                    0,
                                );
                            }

                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
            } else {
                false
            };

            // If there is no content to show, skip presenting entirely.
            if !has_content {
                std::thread::sleep(Duration::from_millis(16));
                continue;
            }

            // Copy staging texture to back buffer.
            // Use CopySubresourceRegion because the staging texture (bitmap size)
            // may differ from the back buffer (window size with borders).
            let back_buffer: Result<ID3D11Texture2D, _> = gfx.swap_chain.GetBuffer(0);
            if let Ok(back_buffer) = back_buffer {
                if let Some(staging) = &gfx.staging_texture {
                    let copy_w = gfx.staging_width.min(gfx.current_width);
                    let copy_h = gfx.staging_height.min(gfx.current_height);
                    if copy_w > 0 && copy_h > 0 {
                        let src_box = D3D11_BOX {
                            left: 0,
                            top: 0,
                            front: 0,
                            right: copy_w,
                            bottom: copy_h,
                            back: 1,
                        };
                        gfx.d3d11_context.CopySubresourceRegion(
                            &back_buffer,
                            0,
                            0,
                            0,
                            0,
                            staging,
                            0,
                            Some(&src_box),
                        );
                    }
                }
            }

            // VSync (SyncInterval=1) provides natural frame pacing.
            let hr = gfx.swap_chain.Present(1, DXGI_PRESENT(0));
            if hr.is_err() {
                error!(hr = ?hr, "Present failed");
            }
        }
    }
}
