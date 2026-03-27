use std::mem::{self, ManuallyDrop};
use std::ffi::c_void;

use windows::Win32::Foundation::BOOL;
use windows::Win32::Graphics::Direct3D::{
    D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT_R32G32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT,
};
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;
use windows::core::s;

use crate::shaders::{compile_shader, blob_as_slice, VERTEX_SHADER_HLSL, PIXEL_SHADER_HLSL};
use crate::state_backup::D3D11StateBackup;

/// Clip-space position + RGBA color vertex.
#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    col: [f32; 4],
}

/// Hardcoded semi-transparent green rectangle in the top-left of the screen.
/// Clip-space: left=-0.95, right=-0.55, top=0.95, bottom=0.75
/// Color: [0.1, 0.8, 0.2, 0.6] (green, 60% opacity)
const QUAD_VERTICES: [Vertex; 6] = {
    let col = [0.1f32, 0.8f32, 0.2f32, 0.6f32];
    let tl = Vertex { pos: [-0.95, 0.95], col };
    let tr = Vertex { pos: [-0.55, 0.95], col };
    let bl = Vertex { pos: [-0.95, 0.75], col };
    let br = Vertex { pos: [-0.55, 0.75], col };
    // Two triangles: TL, TR, BL  and  BL, TR, BR
    [tl, tr, bl, bl, tr, br]
};

pub struct OverlayRenderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    vertex_buffer: ID3D11Buffer,
    blend_state: ID3D11BlendState,
    rasterizer_state: ID3D11RasterizerState,
    render_target_view: Option<ID3D11RenderTargetView>,
}

impl OverlayRenderer {
    /// Initialise all D3D11 resources from a borrowed swap chain pointer.
    ///
    /// # Safety
    /// `swap_chain_ptr` must be a valid `IDXGISwapChain` COM pointer owned by
    /// the hooked game. We wrap it in `ManuallyDrop` so we do not release it.
    pub unsafe fn init(swap_chain_ptr: *mut c_void) -> Result<Self, String> {
        crate::logging::log_to_file("[renderer] init start");

        // ── Borrow the swap chain without taking ownership ────────────────────
        let sc: IDXGISwapChain = mem::transmute_copy(&swap_chain_ptr);
        let sc = ManuallyDrop::new(sc);

        // ── Device + context ──────────────────────────────────────────────────
        let device: ID3D11Device = sc.GetDevice()
            .map_err(|e| format!("GetDevice failed: {e}"))?;

        let context: ID3D11DeviceContext = device.GetImmediateContext()
            .map_err(|e| format!("GetImmediateContext failed: {e}"))?;

        // ── Compile shaders ───────────────────────────────────────────────────
        let vs_blob = compile_shader(VERTEX_SHADER_HLSL, s!("VSMain"), s!("vs_5_0"))
            .map_err(|e| format!("VS compile: {e}"))?;
        let ps_blob = compile_shader(PIXEL_SHADER_HLSL, s!("PSMain"), s!("ps_5_0"))
            .map_err(|e| format!("PS compile: {e}"))?;

        let vs_bytecode = blob_as_slice(&vs_blob);
        let ps_bytecode = blob_as_slice(&ps_blob);

        // ── Vertex shader ─────────────────────────────────────────────────────
        let mut vertex_shader: Option<ID3D11VertexShader> = None;
        device.CreateVertexShader(vs_bytecode, None, Some(&mut vertex_shader))
            .map_err(|e| format!("CreateVertexShader failed: {e}"))?;
        let vertex_shader = vertex_shader.ok_or("CreateVertexShader returned None")?;

        // ── Pixel shader ──────────────────────────────────────────────────────
        let mut pixel_shader: Option<ID3D11PixelShader> = None;
        device.CreatePixelShader(ps_bytecode, None, Some(&mut pixel_shader))
            .map_err(|e| format!("CreatePixelShader failed: {e}"))?;
        let pixel_shader = pixel_shader.ok_or("CreatePixelShader returned None")?;

        // ── Input layout ──────────────────────────────────────────────────────
        let input_elements = [
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("POSITION"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 0,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("COLOR"),
                SemanticIndex: 0,
                Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
                InputSlot: 0,
                AlignedByteOffset: 8, // sizeof([f32; 2]) = 8
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];

        let mut input_layout: Option<ID3D11InputLayout> = None;
        device.CreateInputLayout(&input_elements, vs_bytecode, Some(&mut input_layout))
            .map_err(|e| format!("CreateInputLayout failed: {e}"))?;
        let input_layout = input_layout.ok_or("CreateInputLayout returned None")?;

        // ── Vertex buffer ─────────────────────────────────────────────────────
        let vb_size = (mem::size_of::<Vertex>() * QUAD_VERTICES.len()) as u32;

        let vb_desc = D3D11_BUFFER_DESC {
            ByteWidth: vb_size,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
            StructureByteStride: 0,
        };

        let init_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: QUAD_VERTICES.as_ptr() as *const c_void,
            SysMemPitch: 0,
            SysMemSlicePitch: 0,
        };

        let mut vertex_buffer: Option<ID3D11Buffer> = None;
        device.CreateBuffer(&vb_desc, Some(&init_data), Some(&mut vertex_buffer))
            .map_err(|e| format!("CreateBuffer (vertex) failed: {e}"))?;
        let vertex_buffer = vertex_buffer.ok_or("CreateBuffer returned None")?;

        // ── Blend state — standard alpha blending ─────────────────────────────
        let rt_blend = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: BOOL(1),
            SrcBlend: D3D11_BLEND_SRC_ALPHA,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_ZERO,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };

        let blend_desc = D3D11_BLEND_DESC {
            AlphaToCoverageEnable: BOOL(0),
            IndependentBlendEnable: BOOL(0),
            RenderTarget: [rt_blend; 8],
        };

        let mut blend_state: Option<ID3D11BlendState> = None;
        device.CreateBlendState(&blend_desc, Some(&mut blend_state))
            .map_err(|e| format!("CreateBlendState failed: {e}"))?;
        let blend_state = blend_state.ok_or("CreateBlendState returned None")?;

        // ── Rasterizer state — solid fill, no culling ─────────────────────────
        let rs_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            FrontCounterClockwise: BOOL(0),
            DepthBias: 0,
            DepthBiasClamp: 0.0,
            SlopeScaledDepthBias: 0.0,
            DepthClipEnable: BOOL(1),
            ScissorEnable: BOOL(0),
            MultisampleEnable: BOOL(0),
            AntialiasedLineEnable: BOOL(0),
        };

        let mut rasterizer_state: Option<ID3D11RasterizerState> = None;
        device.CreateRasterizerState(&rs_desc, Some(&mut rasterizer_state))
            .map_err(|e| format!("CreateRasterizerState failed: {e}"))?;
        let rasterizer_state = rasterizer_state.ok_or("CreateRasterizerState returned None")?;

        // ── Render target view from back buffer ───────────────────────────────
        let render_target_view = Some(create_rtv_from_swapchain(&device, &sc)?);

        crate::logging::log_to_file("[renderer] init complete");

        Ok(Self {
            device,
            context,
            vertex_shader,
            pixel_shader,
            input_layout,
            vertex_buffer,
            blend_state,
            rasterizer_state,
            render_target_view,
        })
    }

    /// Render the overlay quad for one frame.
    ///
    /// # Safety
    /// Must be called on the game's render thread with a valid swap chain pointer.
    pub unsafe fn render(&self, swap_chain_ptr: *mut c_void) {
        // ── Save game pipeline state ───────────────────────────────────────────
        let backup = D3D11StateBackup::save(&self.context);

        // ── Get swap chain dimensions for the viewport ────────────────────────
        let sc: IDXGISwapChain = mem::transmute_copy(&swap_chain_ptr);
        let sc = ManuallyDrop::new(sc);

        let desc = match sc.GetDesc() {
            Ok(d) => d,
            Err(e) => {
                crate::logging::log_to_file(&format!("[renderer] GetDesc failed: {e}"));
                backup.restore(&self.context);
                return;
            }
        };

        let width = desc.BufferDesc.Width as f32;
        let height = desc.BufferDesc.Height as f32;

        // ── Set viewport ───────────────────────────────────────────────────────
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: width,
            Height: height,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        self.context.RSSetViewports(Some(&[viewport]));

        // ── Set shaders ────────────────────────────────────────────────────────
        self.context.VSSetShader(&self.vertex_shader, None);
        self.context.PSSetShader(&self.pixel_shader, None);

        // ── Input assembler ────────────────────────────────────────────────────
        self.context.IASetInputLayout(&self.input_layout);
        self.context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

        let stride = mem::size_of::<Vertex>() as u32;
        let offset: u32 = 0;
        self.context.IASetVertexBuffers(
            0,
            1,
            Some(&Some(self.vertex_buffer.clone())),
            Some(&stride),
            Some(&offset),
        );

        // ── Output merger ──────────────────────────────────────────────────────
        self.context.OMSetBlendState(&self.blend_state, Some(&[0.0f32; 4]), 0xFFFF_FFFF);

        if let Some(rtv) = &self.render_target_view {
            self.context.OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
        }

        // ── Rasterizer ─────────────────────────────────────────────────────────
        self.context.RSSetState(&self.rasterizer_state);

        // ── Draw ───────────────────────────────────────────────────────────────
        self.context.Draw(6, 0);

        // ── Restore game state ────────────────────────────────────────────────
        backup.restore(&self.context);
    }

    /// Release the render target view. Call before ResizeBuffers.
    pub fn release_rtv(&mut self) {
        self.render_target_view = None;
        crate::logging::log_to_file("[renderer] RTV released");
    }

    /// Recreate the render target view after ResizeBuffers.
    ///
    /// # Safety
    /// `swap_chain_ptr` must still be a valid `IDXGISwapChain` COM pointer.
    pub unsafe fn recreate_rtv(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        let sc: IDXGISwapChain = mem::transmute_copy(&swap_chain_ptr);
        let sc = ManuallyDrop::new(sc);
        self.render_target_view = Some(create_rtv_from_swapchain(&self.device, &sc)?);
        crate::logging::log_to_file("[renderer] RTV recreated");
        Ok(())
    }
}

/// Helper: get back buffer and create an RTV from it.
unsafe fn create_rtv_from_swapchain(
    device: &ID3D11Device,
    sc: &ManuallyDrop<IDXGISwapChain>,
) -> Result<ID3D11RenderTargetView, String> {
    use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

    let back_buffer: ID3D11Texture2D = sc.GetBuffer(0)
        .map_err(|e| format!("GetBuffer(0) failed: {e}"))?;

    let mut rtv: Option<ID3D11RenderTargetView> = None;
    device.CreateRenderTargetView(&back_buffer, None, Some(&mut rtv))
        .map_err(|e| format!("CreateRenderTargetView failed: {e}"))?;

    rtv.ok_or_else(|| "CreateRenderTargetView returned None".to_string())
}
