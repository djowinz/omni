use std::ffi::c_void;
use std::mem::ManuallyDrop;

use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D1_COLOR_F, D2D1_PIXEL_FORMAT, D2D1_ALPHA_MODE_PREMULTIPLIED,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory1, ID2D1RenderTarget,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_DRAW_TEXT_OPTIONS_NONE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_MEASURING_MODE_NATURAL,
};
use windows::Win32::Graphics::Dxgi::{IDXGISwapChain, IDXGISwapChain3};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Resource,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_BIND_RENDER_TARGET,
};
use windows::Win32::Graphics::Direct3D11on12::{
    D3D11On12CreateDevice, ID3D11On12Device, D3D11_RESOURCE_FLAGS,
};
use windows::Win32::Graphics::Direct3D12::{
    ID3D12Device, ID3D12Resource,
    D3D12_RESOURCE_STATE_PRESENT, D3D12_RESOURCE_STATE_RENDER_TARGET,
};
use windows::core::{w, Interface, IUnknown};

use omni_shared::{ComputedWidget, read_fixed_str};

use crate::logging::log_to_file;

/// Which graphics API the swap chain belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicsApi {
    Unknown,
    DX11,
    DX12,
}

pub struct OverlayRenderer {
    d2d_factory: ID2D1Factory1,
    dwrite_factory: IDWriteFactory,
    render_target: Option<ID2D1RenderTarget>,
    api: GraphicsApi,
    d3d11on12_device: Option<ID3D11On12Device>,
    d3d11_context: Option<ID3D11DeviceContext>,
    wrapped_back_buffer: Option<ID3D11Resource>,
}

impl OverlayRenderer {
    /// Initialize D2D and DirectWrite factories.
    /// The render target is created lazily on first render (needs the swap chain surface).
    pub fn init() -> Result<Self, String> {
        log_to_file("[renderer] initializing D2D1 + DirectWrite");

        let d2d_factory: ID2D1Factory1 = unsafe {
            D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)
                .map_err(|e| format!("D2D1CreateFactory failed: {e}"))?
        };

        let dwrite_factory: IDWriteFactory = unsafe {
            DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)
                .map_err(|e| format!("DWriteCreateFactory failed: {e}"))?
        };

        log_to_file("[renderer] D2D1 + DirectWrite initialized");

        Ok(Self {
            d2d_factory,
            dwrite_factory,
            render_target: None,
            api: GraphicsApi::Unknown,
            d3d11on12_device: None,
            d3d11_context: None,
            wrapped_back_buffer: None,
        })
    }

    /// Detect the graphics API by querying the swap chain's device.
    unsafe fn detect_api(&mut self, sc: &IDXGISwapChain) -> GraphicsApi {
        if self.api != GraphicsApi::Unknown {
            return self.api;
        }

        // Try DX11 first
        if sc.GetDevice::<ID3D11Device>().is_ok() {
            log_to_file("[renderer] detected DX11 swap chain");
            self.api = GraphicsApi::DX11;
            return GraphicsApi::DX11;
        }

        // Try DX12
        if sc.GetDevice::<ID3D12Device>().is_ok() {
            log_to_file("[renderer] detected DX12 swap chain");
            self.api = GraphicsApi::DX12;
            return GraphicsApi::DX12;
        }

        log_to_file("[renderer] could not detect graphics API from swap chain device");
        GraphicsApi::Unknown
    }

    /// Create render target for DX11 swap chains (direct path).
    unsafe fn ensure_render_target_dx11(&mut self, sc: &IDXGISwapChain) -> Result<(), String> {
        let back_buffer: windows::Win32::Graphics::Dxgi::IDXGISurface = sc
            .GetBuffer(0)
            .map_err(|e| format!("GetBuffer(0) failed: {e}"))?;

        let rt = self.create_d2d_render_target(&back_buffer)?;
        self.render_target = Some(rt);

        log_to_file("[renderer] D2D render target created from DX11 swap chain");
        Ok(())
    }

    /// Create render target for DX12 swap chains via D3D11On12.
    /// Unlike DX11, this is called EVERY FRAME because DX12 flip-model swap chains
    /// rotate through multiple back buffers. We must wrap the current buffer each frame.
    unsafe fn ensure_render_target_dx12(&mut self, sc: &IDXGISwapChain) -> Result<(), String> {
        // Create D3D11On12 device if we don't have one yet (survives resize)
        if self.d3d11on12_device.is_none() {
            self.create_d3d11on12_device(sc)?;
        }

        let d3d11on12 = self.d3d11on12_device.as_ref()
            .ok_or_else(|| "D3D11On12 device not available".to_string())?;

        // Get the CURRENT back buffer index — this changes each frame in flip model
        let sc3: IDXGISwapChain3 = sc.cast()
            .map_err(|e| format!("cast to IDXGISwapChain3: {e}"))?;
        let buffer_idx = sc3.GetCurrentBackBufferIndex();

        // Get DX12 back buffer at the current index
        let back_buffer: ID3D12Resource = sc
            .GetBuffer(buffer_idx)
            .map_err(|e| format!("GetBuffer({buffer_idx}) as ID3D12Resource failed: {e}"))?;

        // Wrap DX12 resource as DX11 resource
        let flags = D3D11_RESOURCE_FLAGS {
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            MiscFlags: 0,
            CPUAccessFlags: 0,
            StructureByteStride: 0,
        };

        let mut wrapped: Option<ID3D11Resource> = None;
        d3d11on12.CreateWrappedResource(
            &back_buffer,
            &flags,
            D3D12_RESOURCE_STATE_RENDER_TARGET,
            D3D12_RESOURCE_STATE_PRESENT,
            &mut wrapped,
        ).map_err(|e| format!("CreateWrappedResource failed: {e}"))?;

        let wrapped = wrapped.ok_or_else(|| "CreateWrappedResource returned None".to_string())?;

        // Cast wrapped resource to IDXGISurface
        let surface: windows::Win32::Graphics::Dxgi::IDXGISurface = wrapped.cast()
            .map_err(|e| format!("Cast wrapped resource to IDXGISurface failed: {e}"))?;

        let rt = self.create_d2d_render_target(&surface)?;
        self.render_target = Some(rt);
        self.wrapped_back_buffer = Some(wrapped);

        Ok(())
    }

    /// Create the D3D11On12 device from the DX12 device and captured command queue.
    unsafe fn create_d3d11on12_device(&mut self, sc: &IDXGISwapChain) -> Result<(), String> {
        let dx12_device: ID3D12Device = sc.GetDevice()
            .map_err(|e| format!("GetDevice::<ID3D12Device> failed: {e}"))?;

        // Get the captured command queue from the hook module
        let cmd_queue = crate::hook::CAPTURED_COMMAND_QUEUE.as_ref()
            .ok_or_else(|| "No captured DX12 command queue available".to_string())?;

        let queue_unknown: IUnknown = cmd_queue.cast()
            .map_err(|e| format!("Cast command queue to IUnknown failed: {e}"))?;

        let queues: [Option<IUnknown>; 1] = [Some(queue_unknown)];

        let mut d3d11_device: Option<ID3D11Device> = None;
        let mut d3d11_context: Option<ID3D11DeviceContext> = None;

        D3D11On12CreateDevice(
            &dx12_device,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT.0,
            None,                   // feature levels (use default)
            Some(&queues),          // command queues
            0,                      // node mask
            Some(&mut d3d11_device),
            Some(&mut d3d11_context),
            None,                   // chosen feature level
        ).map_err(|e| format!("D3D11On12CreateDevice failed: {e}"))?;

        let d3d11_device = d3d11_device
            .ok_or_else(|| "D3D11On12CreateDevice returned no device".to_string())?;

        let d3d11on12: ID3D11On12Device = d3d11_device.cast()
            .map_err(|e| format!("Cast ID3D11Device to ID3D11On12Device failed: {e}"))?;

        self.d3d11on12_device = Some(d3d11on12);
        self.d3d11_context = d3d11_context;

        log_to_file("[renderer] D3D11On12 device created");
        Ok(())
    }

    /// Create a D2D render target from a DXGI surface.
    unsafe fn create_d2d_render_target(
        &self,
        surface: &windows::Win32::Graphics::Dxgi::IDXGISurface,
    ) -> Result<ID2D1RenderTarget, String> {
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            r#type: windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_TYPE_DEFAULT,
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_UNKNOWN,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            dpiX: 0.0,
            dpiY: 0.0,
            usage: windows::Win32::Graphics::Direct2D::D2D1_RENDER_TARGET_USAGE_NONE,
            minLevel: windows::Win32::Graphics::Direct2D::D2D1_FEATURE_LEVEL_DEFAULT,
        };

        self.d2d_factory
            .CreateDxgiSurfaceRenderTarget(surface, &props)
            .map_err(|e| format!("CreateDxgiSurfaceRenderTarget failed: {e}"))
    }

    /// Ensure we have a render target for the current swap chain back buffer.
    /// For DX11: cached (created once, recreated on resize).
    /// For DX12: recreated every frame (back buffer index rotates in flip model).
    unsafe fn ensure_render_target(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        let sc: IDXGISwapChain = std::mem::transmute_copy(&swap_chain_ptr);
        let sc = ManuallyDrop::new(sc);

        let api = self.detect_api(&sc);

        match api {
            GraphicsApi::DX11 => {
                if self.render_target.is_some() {
                    return Ok(());
                }
                self.ensure_render_target_dx11(&sc)
            }
            GraphicsApi::DX12 => {
                // Release previous frame's resources — buffer index has rotated
                self.render_target = None;
                self.wrapped_back_buffer = None;
                self.ensure_render_target_dx12(&sc)
            }
            GraphicsApi::Unknown => Err("Unknown graphics API — cannot create render target".to_string()),
        }
    }

    /// Acquire wrapped DX12 resources before rendering. No-op for DX11.
    unsafe fn acquire_wrapped_resource(&self) {
        if self.api != GraphicsApi::DX12 {
            return;
        }
        if let (Some(d3d11on12), Some(wrapped)) =
            (&self.d3d11on12_device, &self.wrapped_back_buffer)
        {
            d3d11on12.AcquireWrappedResources(&[Some(wrapped.clone())]);
        }
    }

    /// Release wrapped DX12 resources after rendering + flush. No-op for DX11.
    unsafe fn release_wrapped_resource(&self) {
        if self.api != GraphicsApi::DX12 {
            return;
        }
        if let (Some(d3d11on12), Some(wrapped)) =
            (&self.d3d11on12_device, &self.wrapped_back_buffer)
        {
            d3d11on12.ReleaseWrappedResources(&[Some(wrapped.clone())]);
        }
        if let Some(ctx) = &self.d3d11_context {
            ctx.Flush();
        }
    }

    /// Render a list of computed widgets onto the swap chain back buffer.
    pub unsafe fn render(&mut self, swap_chain_ptr: *mut c_void, widgets: &[ComputedWidget]) {
        if let Err(e) = self.ensure_render_target(swap_chain_ptr) {
            log_to_file(&format!("[renderer] failed to create render target: {e}"));
            return;
        }

        let rt = match &self.render_target {
            Some(rt) => rt,
            None => return,
        };

        self.acquire_wrapped_resource();

        rt.BeginDraw();

        for widget in widgets {
            if widget.opacity <= 0.0 {
                continue;
            }

            let rect = D2D_RECT_F {
                left: widget.x,
                top: widget.y,
                right: widget.x + widget.width,
                bottom: widget.y + widget.height,
            };

            // Draw background
            let bg = &widget.bg_color_rgba;
            if bg[3] > 0 {
                let bg_color = D2D1_COLOR_F {
                    r: bg[0] as f32 / 255.0,
                    g: bg[1] as f32 / 255.0,
                    b: bg[2] as f32 / 255.0,
                    a: (bg[3] as f32 / 255.0) * widget.opacity,
                };

                if let Ok(brush) = rt.CreateSolidColorBrush(&bg_color, None) {
                    let radius = widget.border_radius[0]; // simplified: use top-left for all
                    if radius > 0.0 {
                        let rounded = D2D1_ROUNDED_RECT {
                            rect,
                            radiusX: radius,
                            radiusY: radius,
                        };
                        rt.FillRoundedRectangle(&rounded, &brush);
                    } else {
                        rt.FillRectangle(&rect, &brush);
                    }
                }
            }

            // Draw text
            let text = read_fixed_str(&widget.format_pattern);
            if text.is_empty() {
                continue;
            }

            let font_weight = if widget.font_weight >= 700 {
                DWRITE_FONT_WEIGHT_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            };

            let text_format = self.dwrite_factory.CreateTextFormat(
                w!("Segoe UI"),
                None,
                font_weight,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                widget.font_size,
                w!("en-us"),
            );

            let text_format = match text_format {
                Ok(tf) => tf,
                Err(_) => continue,
            };

            let _ = text_format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);

            let fg = &widget.color_rgba;
            let fg_color = D2D1_COLOR_F {
                r: fg[0] as f32 / 255.0,
                g: fg[1] as f32 / 255.0,
                b: fg[2] as f32 / 255.0,
                a: (fg[3] as f32 / 255.0) * widget.opacity,
            };

            if let Ok(brush) = rt.CreateSolidColorBrush(&fg_color, None) {
                let text_wide: Vec<u16> = text.encode_utf16().collect();
                rt.DrawText(
                    &text_wide,
                    &text_format,
                    &rect,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                    DWRITE_MEASURING_MODE_NATURAL,
                );
            }
        }

        let _ = rt.EndDraw(None, None);

        self.release_wrapped_resource();
    }

    /// Release the render target. Call before ResizeBuffers.
    pub fn release_render_target(&mut self) {
        self.render_target = None;
        self.wrapped_back_buffer = None;
        log_to_file("[renderer] D2D render target released");
    }

    /// Recreate render target after ResizeBuffers.
    pub unsafe fn recreate_render_target(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        self.render_target = None;
        self.wrapped_back_buffer = None;
        self.ensure_render_target(swap_chain_ptr)
    }
}
