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
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_UNKNOWN;
use windows::core::w;

use omni_shared::{ComputedWidget, read_fixed_str};

use crate::logging::log_to_file;

pub struct OverlayRenderer {
    d2d_factory: ID2D1Factory1,
    dwrite_factory: IDWriteFactory,
    render_target: Option<ID2D1RenderTarget>,
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
        })
    }

    /// Ensure we have a render target for the current swap chain back buffer.
    unsafe fn ensure_render_target(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        if self.render_target.is_some() {
            return Ok(());
        }

        let sc: IDXGISwapChain = std::mem::transmute_copy(&swap_chain_ptr);
        let sc = ManuallyDrop::new(sc);

        let back_buffer: windows::Win32::Graphics::Dxgi::IDXGISurface = sc
            .GetBuffer(0)
            .map_err(|e| format!("GetBuffer(0) failed: {e}"))?;

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

        let rt = self.d2d_factory
            .CreateDxgiSurfaceRenderTarget(&back_buffer, &props)
            .map_err(|e| format!("CreateDxgiSurfaceRenderTarget failed: {e}"))?;

        self.render_target = Some(rt);

        log_to_file("[renderer] D2D render target created from swap chain");
        Ok(())
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
    }

    /// Release the render target. Call before ResizeBuffers.
    pub fn release_render_target(&mut self) {
        self.render_target = None;
        log_to_file("[renderer] D2D render target released");
    }

    /// Recreate render target after ResizeBuffers.
    pub unsafe fn recreate_render_target(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        self.render_target = None;
        self.ensure_render_target(swap_chain_ptr)
    }
}
