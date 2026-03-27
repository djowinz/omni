use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

pub struct D3D11StateBackup {
    pub viewports_count: u32,
    pub scissor_rects_count: u32,
    pub viewports: [D3D11_VIEWPORT; 16],
    pub scissor_rects: [RECT; 16],
    pub rasterizer_state: Option<ID3D11RasterizerState>,
    pub blend_state: Option<ID3D11BlendState>,
    pub blend_factor: [f32; 4],
    pub sample_mask: u32,
    pub depth_stencil_state: Option<ID3D11DepthStencilState>,
    pub stencil_ref: u32,
    pub ps: Option<ID3D11PixelShader>,
    pub vs: Option<ID3D11VertexShader>,
    pub primitive_topology: D3D_PRIMITIVE_TOPOLOGY,
    pub input_layout: Option<ID3D11InputLayout>,
    pub vertex_buffer: Option<ID3D11Buffer>,
    pub vertex_buffer_stride: u32,
    pub vertex_buffer_offset: u32,
    pub index_buffer: Option<ID3D11Buffer>,
    pub index_buffer_format: DXGI_FORMAT,
    pub index_buffer_offset: u32,
    pub render_target: Option<ID3D11RenderTargetView>,
    pub depth_stencil_view: Option<ID3D11DepthStencilView>,
}

impl D3D11StateBackup {
    pub unsafe fn save(ctx: &ID3D11DeviceContext) -> Self {
        let mut backup = Self {
            viewports_count: 16,
            scissor_rects_count: 16,
            viewports: [D3D11_VIEWPORT::default(); 16],
            scissor_rects: [RECT::default(); 16],
            rasterizer_state: None,
            blend_state: None,
            blend_factor: [0.0f32; 4],
            sample_mask: 0,
            depth_stencil_state: None,
            stencil_ref: 0,
            ps: None,
            vs: None,
            primitive_topology: D3D_PRIMITIVE_TOPOLOGY::default(),
            input_layout: None,
            vertex_buffer: None,
            vertex_buffer_stride: 0,
            vertex_buffer_offset: 0,
            index_buffer: None,
            index_buffer_format: DXGI_FORMAT::default(),
            index_buffer_offset: 0,
            render_target: None,
            depth_stencil_view: None,
        };

        // Viewports and scissor rects
        // RSGetViewports: (*mut u32, Option<*mut D3D11_VIEWPORT>)
        ctx.RSGetViewports(
            &mut backup.viewports_count,
            Some(backup.viewports.as_mut_ptr()),
        );
        ctx.RSGetScissorRects(
            &mut backup.scissor_rects_count,
            Some(backup.scissor_rects.as_mut_ptr()),
        );
        backup.rasterizer_state = ctx.RSGetState().ok();

        // Blend + depth stencil
        // OMGetBlendState: (Option<*mut Option<T>>, Option<&mut [f32;4]>, Option<*mut u32>)
        ctx.OMGetBlendState(
            Some(&mut backup.blend_state),
            Some(&mut backup.blend_factor),
            Some(&mut backup.sample_mask),
        );
        ctx.OMGetDepthStencilState(
            Some(&mut backup.depth_stencil_state),
            Some(&mut backup.stencil_ref),
        );

        // Shaders
        // PSGetShader: (*mut Option<T>, Option<*mut Option<ID3D11ClassInstance>>, Option<*mut u32>)
        let mut ps_count: u32 = 0;
        ctx.PSGetShader(&mut backup.ps, None, Some(&mut ps_count));
        let mut vs_count: u32 = 0;
        ctx.VSGetShader(&mut backup.vs, None, Some(&mut vs_count));

        // Input assembler
        // IAGetPrimitiveTopology: returns D3D_PRIMITIVE_TOPOLOGY directly
        backup.primitive_topology = ctx.IAGetPrimitiveTopology();
        // IAGetInputLayout: returns Result<ID3D11InputLayout>
        backup.input_layout = ctx.IAGetInputLayout().ok();

        // Vertex buffer slot 0
        // IAGetVertexBuffers: (u32, u32, Option<*mut Option<T>>, Option<*mut u32>, Option<*mut u32>)
        ctx.IAGetVertexBuffers(
            0,
            1,
            Some(&mut backup.vertex_buffer),
            Some(&mut backup.vertex_buffer_stride),
            Some(&mut backup.vertex_buffer_offset),
        );

        // Index buffer
        // IAGetIndexBuffer: (Option<*mut Option<T>>, Option<*mut DXGI_FORMAT>, Option<*mut u32>)
        ctx.IAGetIndexBuffer(
            Some(&mut backup.index_buffer),
            Some(&mut backup.index_buffer_format),
            Some(&mut backup.index_buffer_offset),
        );

        // Render targets
        // OMGetRenderTargets: (Option<&mut [Option<T>]>, Option<*mut Option<T>>)
        let mut rtvs: [Option<ID3D11RenderTargetView>; 1] = [None];
        ctx.OMGetRenderTargets(Some(&mut rtvs), Some(&mut backup.depth_stencil_view));
        backup.render_target = rtvs[0].take();

        backup
    }

    pub unsafe fn restore(self, ctx: &ID3D11DeviceContext) {
        ctx.RSSetViewports(Some(&self.viewports[..self.viewports_count as usize]));
        ctx.RSSetScissorRects(Some(&self.scissor_rects[..self.scissor_rects_count as usize]));
        // RSSetState: P0: Param<ID3D11RasterizerState> — Option<&T> implements Param<T>
        ctx.RSSetState(self.rasterizer_state.as_ref());

        // OMSetBlendState: P0: Param<T>, Option<&[f32;4]>, u32
        ctx.OMSetBlendState(self.blend_state.as_ref(), Some(&self.blend_factor), self.sample_mask);
        // OMSetDepthStencilState: P0: Param<T>, u32
        ctx.OMSetDepthStencilState(self.depth_stencil_state.as_ref(), self.stencil_ref);

        // PSSetShader / VSSetShader: P0: Param<T>, Option<&[Option<T>]>
        ctx.PSSetShader(self.ps.as_ref(), None);
        ctx.VSSetShader(self.vs.as_ref(), None);

        ctx.IASetPrimitiveTopology(self.primitive_topology);
        // IASetInputLayout: P0: Param<T>
        ctx.IASetInputLayout(self.input_layout.as_ref());

        // IASetVertexBuffers: (u32, u32, Option<*const Option<T>>, Option<*const u32>, Option<*const u32>)
        ctx.IASetVertexBuffers(
            0,
            1,
            Some(&self.vertex_buffer),
            Some(&self.vertex_buffer_stride),
            Some(&self.vertex_buffer_offset),
        );
        // IASetIndexBuffer: P0: Param<T>, DXGI_FORMAT, u32
        ctx.IASetIndexBuffer(self.index_buffer.as_ref(), self.index_buffer_format, self.index_buffer_offset);

        // OMSetRenderTargets: Option<&[Option<T>]>, P0: Param<T>
        let rtvs: [Option<ID3D11RenderTargetView>; 1] = [self.render_target];
        ctx.OMSetRenderTargets(Some(&rtvs), self.depth_stencil_view.as_ref());
    }
}
