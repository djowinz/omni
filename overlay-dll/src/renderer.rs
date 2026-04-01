use std::ffi::c_void;
use std::mem::ManuallyDrop;

use omni_shared::{ComputedWidget, read_fixed_str};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D_RECT_F, D2D_POINT_2F, D2D_SIZE_F, D2D1_COLOR_F, D2D1_GRADIENT_STOP,
    D2D1_PIXEL_FORMAT, D2D1_ALPHA_MODE_PREMULTIPLIED,
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D1_FILL_MODE_WINDING,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory1, ID2D1RenderTarget, ID2D1Brush,
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_ROUNDED_RECT, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP,
    D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL, D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_BOLD,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_LEADING, DWRITE_MEASURING_MODE_NATURAL,
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

use crate::logging::log_to_file;

/// Compute gradient start/end points from a CSS-style angle and a bounding rect.
/// CSS gradient angles: 0deg = to top, 90deg = to right, 180deg = to bottom.
fn gradient_points(rect: &D2D_RECT_F, angle_deg: f32) -> (D2D_POINT_2F, D2D_POINT_2F) {
    let cx = (rect.left + rect.right) / 2.0;
    let cy = (rect.top + rect.bottom) / 2.0;
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;

    // CSS gradient angle: 0deg = to top, 90deg = to right, 180deg = to bottom
    let rad = (angle_deg - 90.0_f32).to_radians();
    let dx = rad.cos() * w / 2.0;
    let dy = rad.sin() * h / 2.0;

    let start = D2D_POINT_2F { x: cx - dx, y: cy - dy };
    let end = D2D_POINT_2F { x: cx + dx, y: cy + dy };
    (start, end)
}

/// Fill a rectangle with per-corner border radii using `ID2D1PathGeometry`.
///
/// `radii` order: [top-left, top-right, bottom-right, bottom-left].
/// If all four corners are equal, uses the fast `FillRoundedRectangle` path.
/// Otherwise builds a path geometry with arc segments for each corner.
unsafe fn fill_rounded_rect_per_corner(
    rt: &ID2D1RenderTarget,
    factory: &ID2D1Factory1,
    rect: &D2D_RECT_F,
    radii: [f32; 4],
    brush: &ID2D1Brush,
) {
    let [r_tl, r_tr, r_br, r_bl] = radii;

    // Fast path: uniform radius
    if r_tl == r_tr && r_tr == r_br && r_br == r_bl {
        if r_tl > 0.0 {
            let rounded = D2D1_ROUNDED_RECT {
                rect: *rect,
                radiusX: r_tl,
                radiusY: r_tl,
            };
            let _ = rt.FillRoundedRectangle(&rounded, brush);
        } else {
            let _ = rt.FillRectangle(rect, brush);
        }
        return;
    }

    // Slow path: build an ID2D1PathGeometry with per-corner arcs
    let geometry = match factory.CreatePathGeometry() {
        Ok(g) => g,
        Err(_) => {
            // Fallback: fill plain rectangle
            let _ = rt.FillRectangle(rect, brush);
            return;
        }
    };

    let sink = match geometry.Open() {
        Ok(s) => s,
        Err(_) => {
            let _ = rt.FillRectangle(rect, brush);
            return;
        }
    };

    sink.SetFillMode(D2D1_FILL_MODE_WINDING);

    let left = rect.left;
    let top = rect.top;
    let right = rect.right;
    let bottom = rect.bottom;

    // Start at (left, top + r_tl) — left edge just below the top-left corner
    sink.BeginFigure(
        D2D_POINT_2F { x: left, y: top + r_tl },
        D2D1_FIGURE_BEGIN_FILLED,
    );

    // Top-left corner arc: from (left, top+r_tl) to (left+r_tl, top)
    if r_tl > 0.0 {
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: D2D_POINT_2F { x: left + r_tl, y: top },
            size: D2D_SIZE_F { width: r_tl, height: r_tl },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        });
    } else {
        sink.AddLine(D2D_POINT_2F { x: left, y: top });
    }

    // Top edge to (right - r_tr, top)
    sink.AddLine(D2D_POINT_2F { x: right - r_tr, y: top });

    // Top-right corner arc: from (right-r_tr, top) to (right, top+r_tr)
    if r_tr > 0.0 {
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: D2D_POINT_2F { x: right, y: top + r_tr },
            size: D2D_SIZE_F { width: r_tr, height: r_tr },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        });
    } else {
        sink.AddLine(D2D_POINT_2F { x: right, y: top });
    }

    // Right edge to (right, bottom - r_br)
    sink.AddLine(D2D_POINT_2F { x: right, y: bottom - r_br });

    // Bottom-right corner arc: from (right, bottom-r_br) to (right-r_br, bottom)
    if r_br > 0.0 {
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: D2D_POINT_2F { x: right - r_br, y: bottom },
            size: D2D_SIZE_F { width: r_br, height: r_br },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        });
    } else {
        sink.AddLine(D2D_POINT_2F { x: right, y: bottom });
    }

    // Bottom edge to (left + r_bl, bottom)
    sink.AddLine(D2D_POINT_2F { x: left + r_bl, y: bottom });

    // Bottom-left corner arc: from (left+r_bl, bottom) to (left, bottom-r_bl)
    if r_bl > 0.0 {
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: D2D_POINT_2F { x: left, y: bottom - r_bl },
            size: D2D_SIZE_F { width: r_bl, height: r_bl },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: D2D1_ARC_SIZE_SMALL,
        });
    } else {
        sink.AddLine(D2D_POINT_2F { x: left, y: bottom });
    }

    // Close the figure (implicit line back to start point)
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    let _ = sink.Close();

    let _ = rt.FillGeometry(&geometry, brush, None::<&ID2D1Brush>);
}

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
    /// Tracks the last swap chain pointer to detect when the game creates a new one.
    last_swap_chain_ptr: *mut c_void,
    /// Number of consecutive DX12 render failures. After too many, stop trying
    /// until the swap chain changes (avoids spamming errors during splash screens).
    dx12_fail_count: u32,
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
            last_swap_chain_ptr: std::ptr::null_mut(),
            dx12_fail_count: 0,
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
        if let Ok(dx12_device) = sc.GetDevice::<ID3D12Device>() {
            log_to_file("[renderer] detected DX12 swap chain");
            self.api = GraphicsApi::DX12;

            // Set up ExecuteCommandLists hook now that we have the game's D3D12 device.
            // This is deferred from install_hooks to avoid racing with game's D3D12 init.
            if let Err(e) = crate::hook::hook_execute_command_lists_deferred(&dx12_device) {
                log_to_file(&format!("[renderer] WARNING: failed to hook ExecuteCommandLists: {e}"));
            }

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

    /// Create the D3D11On12 device from the DX12 device and the captured command queue.
    /// The queue is captured by the ExecuteCommandLists hook (fires every frame)
    /// or the CreateSwapChainForHwnd hook (fires on swap chain creation).
    unsafe fn create_d3d11on12_device(&mut self, sc: &IDXGISwapChain) -> Result<(), String> {
        let dx12_device: ID3D12Device = sc.GetDevice()
            .map_err(|e| format!("GetDevice::<ID3D12Device> failed: {e}"))?;

        let cmd_queue = crate::hook::CAPTURED_COMMAND_QUEUE.as_ref()
            .ok_or_else(|| "DX12 command queue not yet captured — waiting for ExecuteCommandLists hook".to_string())?;

        // Verify the captured queue is from the same device as the swap chain
        let mut queue_device: Option<ID3D12Device> = None;
        let device_check = cmd_queue.GetDevice(&mut queue_device);
        match (device_check, &queue_device) {
            (Ok(()), Some(qd)) => {
                let same_device = Interface::as_raw(qd) == Interface::as_raw(&dx12_device);
                if !same_device {
                    // Queue is from a different device (e.g. splash screen).
                    // Don't increment fail count — the ExecuteCommandLists hook
                    // will overwrite with the correct queue shortly.
                    return Err("Queue device mismatch — waiting for correct queue".into());
                }
                log_to_file("[renderer] using captured command queue for D3D11On12 (device verified)");
            }
            _ => {
                log_to_file("[renderer] WARNING: could not verify queue device, proceeding anyway");
            }
        }

        let queue_unknown: IUnknown = cmd_queue.cast()
            .map_err(|e| format!("Cast captured command queue to IUnknown: {e}"))?;

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

    /// Reset all cached state. Called when the swap chain pointer changes
    /// (game transitioned from splash screen to main renderer, or recreated the swap chain).
    fn reset_state(&mut self) {
        self.render_target = None;
        self.wrapped_back_buffer = None;
        self.d3d11on12_device = None;
        self.d3d11_context = None;
        self.api = GraphicsApi::Unknown;
        self.dx12_fail_count = 0;
        log_to_file("[renderer] swap chain changed — reset all state");
    }

    /// Ensure we have a render target for the current swap chain back buffer.
    /// For DX11: cached (created once, recreated on resize).
    /// For DX12: recreated every frame (back buffer index rotates in flip model).
    unsafe fn ensure_render_target(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        // Detect swap chain changes (splash → game transition, recreation)
        if swap_chain_ptr != self.last_swap_chain_ptr {
            if !self.last_swap_chain_ptr.is_null() {
                self.reset_state();
            }
            self.last_swap_chain_ptr = swap_chain_ptr;
        }

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
                // If we've failed too many times on this swap chain, stop trying
                // until the swap chain changes (avoids spamming during splash screens).
                // Allow more attempts (10) to give ExecuteCommandLists hook time to
                // capture the command queue after re-injection.
                if self.dx12_fail_count >= 10 {
                    return Err("DX12 rendering suspended after repeated failures".into());
                }
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
            if self.api == GraphicsApi::DX12 {
                // Don't count device-mismatch toward suspension — the hooks will
                // update the captured queue and it'll resolve itself.
                if !e.contains("device mismatch") && !e.contains("not yet captured") {
                    self.dx12_fail_count += 1;
                    if self.dx12_fail_count == 10 {
                        log_to_file(&format!(
                            "[renderer] DX12 rendering suspended after 10 failures (last: {e}). \
                             Will retry when swap chain changes."
                        ));
                    }
                }
            }
            return;
        }

        // DX12 render succeeded — reset fail counter
        if self.api == GraphicsApi::DX12 {
            self.dx12_fail_count = 0;
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

            let radii = widget.border_radius; // [tl, tr, br, bl]

            // Draw box shadow (before main background)
            if widget.box_shadow.enabled {
                let shadow = &widget.box_shadow;
                let sc = &shadow.color_rgba;
                let shadow_alpha = (sc[3] as f32 / 255.0) * widget.opacity;

                if shadow_alpha > 0.0 {
                    let shadow_rect = D2D_RECT_F {
                        left: rect.left + shadow.offset_x,
                        top: rect.top + shadow.offset_y,
                        right: rect.right + shadow.offset_x,
                        bottom: rect.bottom + shadow.offset_y,
                    };

                    // Smooth shadow: draw from outermost (faintest) to innermost (darkest)
                    // using a Gaussian-like falloff. More passes = smoother gradient.
                    let blur = shadow.blur_radius.max(0.0);
                    if blur > 0.0 {
                        let num_passes = ((blur / 2.0).ceil() as u32).clamp(4, 20);
                        // Draw outermost first (painter's algorithm)
                        for pass in (0..num_passes).rev() {
                            let t = (pass as f32 + 1.0) / num_passes as f32; // 1.0 (outermost) to ~0 (innermost)
                            let expand = blur * t;
                            // Gaussian-like falloff: exp(-3 * t^2)
                            let alpha_factor = (-3.0 * t * t).exp();
                            // Each pass contributes a fraction of the total alpha
                            let pass_alpha = shadow_alpha * alpha_factor / num_passes as f32;

                            let pass_rect = D2D_RECT_F {
                                left: shadow_rect.left - expand,
                                top: shadow_rect.top - expand,
                                right: shadow_rect.right + expand,
                                bottom: shadow_rect.bottom + expand,
                            };

                            let shadow_color = D2D1_COLOR_F {
                                r: sc[0] as f32 / 255.0,
                                g: sc[1] as f32 / 255.0,
                                b: sc[2] as f32 / 255.0,
                                a: pass_alpha,
                            };

                            // Scale radii with expansion
                            let pass_radii = [
                                radii[0] + expand,
                                radii[1] + expand,
                                radii[2] + expand,
                                radii[3] + expand,
                            ];

                            if let Ok(shadow_brush) = rt.CreateSolidColorBrush(&shadow_color, None) {
                                fill_rounded_rect_per_corner(
                                    rt, &self.d2d_factory, &pass_rect, pass_radii, &*shadow_brush,
                                );
                            }
                        }
                    } else {
                        // No blur — single sharp shadow
                        let shadow_color = D2D1_COLOR_F {
                            r: sc[0] as f32 / 255.0,
                            g: sc[1] as f32 / 255.0,
                            b: sc[2] as f32 / 255.0,
                            a: shadow_alpha,
                        };
                        if let Ok(shadow_brush) = rt.CreateSolidColorBrush(&shadow_color, None) {
                            fill_rounded_rect_per_corner(
                                rt, &self.d2d_factory, &shadow_rect, radii, &*shadow_brush,
                            );
                        }
                    }
                }
            }

            // Draw background (gradient or solid color) with per-corner border radius
            if widget.bg_gradient.enabled {
                // Linear gradient background
                let grad = &widget.bg_gradient;
                let start_color = D2D1_COLOR_F {
                    r: grad.start_rgba[0] as f32 / 255.0,
                    g: grad.start_rgba[1] as f32 / 255.0,
                    b: grad.start_rgba[2] as f32 / 255.0,
                    a: (grad.start_rgba[3] as f32 / 255.0) * widget.opacity,
                };
                let end_color = D2D1_COLOR_F {
                    r: grad.end_rgba[0] as f32 / 255.0,
                    g: grad.end_rgba[1] as f32 / 255.0,
                    b: grad.end_rgba[2] as f32 / 255.0,
                    a: (grad.end_rgba[3] as f32 / 255.0) * widget.opacity,
                };

                let stops = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: start_color },
                    D2D1_GRADIENT_STOP { position: 1.0, color: end_color },
                ];

                if let Ok(stop_collection) = rt.CreateGradientStopCollection(
                    &stops,
                    D2D1_GAMMA_2_2,
                    D2D1_EXTEND_MODE_CLAMP,
                ) {
                    let (start_pt, end_pt) = gradient_points(&rect, grad.angle_deg);
                    let grad_props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: start_pt,
                        endPoint: end_pt,
                    };

                    if let Ok(brush) = rt.CreateLinearGradientBrush(
                        &grad_props,
                        None,
                        &stop_collection,
                    ) {
                        fill_rounded_rect_per_corner(
                            rt, &self.d2d_factory, &rect, radii, &*brush,
                        );
                    }
                }
            } else {
                // Solid color background (default)
                let bg = &widget.bg_color_rgba;
                if bg[3] > 0 {
                    let bg_color = D2D1_COLOR_F {
                        r: bg[0] as f32 / 255.0,
                        g: bg[1] as f32 / 255.0,
                        b: bg[2] as f32 / 255.0,
                        a: (bg[3] as f32 / 255.0) * widget.opacity,
                    };

                    if let Ok(brush) = rt.CreateSolidColorBrush(&bg_color, None) {
                        fill_rounded_rect_per_corner(
                            rt, &self.d2d_factory, &rect, radii, &*brush,
                        );
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
            let _ = text_format.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_LEADING);

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
        // For DX12, also reset the D3D11On12 device since the swap chain
        // buffers are being recreated and old wrapped resources are invalid.
        if self.api == GraphicsApi::DX12 {
            self.d3d11on12_device = None;
            self.d3d11_context = None;
            self.dx12_fail_count = 0;
        }
        log_to_file("[renderer] D2D render target released");
    }

    /// Recreate render target after ResizeBuffers.
    pub unsafe fn recreate_render_target(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        self.render_target = None;
        self.wrapped_back_buffer = None;
        self.ensure_render_target(swap_chain_ptr)
    }
}
