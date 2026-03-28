# Phase 3: Render a Colored Rectangle Overlay

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Use the hooked `Present` to acquire the game's D3D11 device, create a render target view from the back buffer, and draw a semi-transparent colored rectangle on screen as a proof-of-concept overlay.

**Architecture:** On the first `hooked_present` call, lazy-initialize an `OverlayRenderer` that creates D3D11 resources (shaders, vertex buffer, blend state, rasterizer state). Each frame, it saves the game's pipeline state, binds its own resources, draws a quad, and restores the game's state. The renderer is stored in a global `OnceLock`. On `ResizeBuffers`, the render target view is released before the resize and recreated after.

**Tech Stack:** Rust, `windows` crate 0.58 (D3D11 + DXGI + Fxc for shader compilation), `minhook` (already installed), HLSL shaders compiled at runtime via `D3DCompile`.

**Testing notes:** The overlay renderer has limited unit-testability since it requires a real D3D11 device. Integration testing is manual: inject into a DX11 game and visually confirm a semi-transparent rectangle appears. The state backup/restore struct can be tested for correct default initialization.

**Depends on:** Phase 2 complete (Present hook firing every frame).

---

## File Map

```
overlay-dll/
  Cargo.toml                         # Add Win32_Graphics_Direct3D_Fxc feature
  src/
    lib.rs                           # Unchanged
    logging.rs                       # Unchanged
    hook.rs                          # Unchanged
    present.rs                       # Modified: call renderer from hooked_present/resize
    renderer.rs                      # NEW: OverlayRenderer — resource creation, draw, state mgmt
    state_backup.rs                  # NEW: D3D11 pipeline state save/restore
    shaders.rs                       # NEW: HLSL source strings + compile helper
```

---

### Task 1: Add Fxc Feature + Create Module Stubs

**Files:**
- Modify: `overlay-dll/Cargo.toml`
- Modify: `overlay-dll/src/lib.rs`
- Create: `overlay-dll/src/renderer.rs`
- Create: `overlay-dll/src/state_backup.rs`
- Create: `overlay-dll/src/shaders.rs`

- [ ] **Step 1: Add the Fxc feature to Cargo.toml**

Add `"Win32_Graphics_Direct3D_Fxc"` to the windows features list in `overlay-dll/Cargo.toml`:

```toml
[dependencies.windows]
version = "0.58"
features = [
    "Win32_Foundation",
    "Win32_System_SystemServices",
    "Win32_System_LibraryLoader",
    "Win32_Graphics_Direct3D",
    "Win32_Graphics_Direct3D_Fxc",
    "Win32_Graphics_Direct3D11",
    "Win32_Graphics_Dxgi",
    "Win32_Graphics_Dxgi_Common",
    "Win32_Graphics_Gdi",
    "Win32_UI_WindowsAndMessaging",
]
```

- [ ] **Step 2: Add module declarations to lib.rs**

Add three new module declarations after the existing ones in `overlay-dll/src/lib.rs`:

```rust
mod logging;
mod hook;
mod present;
mod shaders;
mod state_backup;
mod renderer;
```

- [ ] **Step 3: Create stub files**

Create `overlay-dll/src/shaders.rs`:
```rust
// HLSL shader sources and compilation helper — implemented in Task 2.
```

Create `overlay-dll/src/state_backup.rs`:
```rust
// D3D11 pipeline state save/restore — implemented in Task 3.
```

Create `overlay-dll/src/renderer.rs`:
```rust
// Overlay renderer — implemented in Task 4.
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles with no errors.

- [ ] **Step 5: Commit**

```bash
git add overlay-dll/Cargo.toml overlay-dll/src/lib.rs overlay-dll/src/shaders.rs overlay-dll/src/state_backup.rs overlay-dll/src/renderer.rs
git commit -m "feat(overlay-dll): add Fxc feature and module stubs for Phase 3 renderer"
```

---

### Task 2: HLSL Shaders and Compile Helper

**Files:**
- Modify: `overlay-dll/src/shaders.rs`

This task provides the HLSL shader source code and a `compile_shader` function that calls `D3DCompile` at runtime.

- [ ] **Step 1: Implement shaders.rs**

```rust
use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::core::{s, PCSTR};

/// Minimal vertex shader: takes clip-space position + RGBA color, passes through.
pub const VERTEX_SHADER_HLSL: &str = r"
struct VS_INPUT {
    float2 pos : POSITION;
    float4 col : COLOR;
};
struct PS_INPUT {
    float4 pos : SV_POSITION;
    float4 col : COLOR;
};
PS_INPUT VSMain(VS_INPUT input) {
    PS_INPUT output;
    output.pos = float4(input.pos, 0.0, 1.0);
    output.col = input.col;
    return output;
}
";

/// Minimal pixel shader: outputs the interpolated color.
pub const PIXEL_SHADER_HLSL: &str = r"
struct PS_INPUT {
    float4 pos : SV_POSITION;
    float4 col : COLOR;
};
float4 PSMain(PS_INPUT input) : SV_TARGET {
    return input.col;
}
";

/// Compile an HLSL shader source string to bytecode using D3DCompile.
///
/// # Arguments
/// * `source` - HLSL source code
/// * `entry_point` - Entry function name (e.g. s!("VSMain"))
/// * `target` - Shader model (e.g. s!("vs_4_0"))
///
/// # Safety
/// Calls the D3DCompile FFI function.
pub unsafe fn compile_shader(
    source: &str,
    entry_point: PCSTR,
    target: PCSTR,
) -> Result<ID3DBlob, String> {
    let mut blob: Option<ID3DBlob> = None;
    let mut error_blob: Option<ID3DBlob> = None;

    let result = D3DCompile(
        source.as_ptr() as *const _,
        source.len(),
        None,
        None,
        None,
        entry_point,
        target,
        0,
        0,
        &mut blob,
        Some(&mut error_blob),
    );

    if let Err(e) = result {
        let msg = if let Some(ref err) = error_blob {
            let ptr = err.GetBufferPointer() as *const u8;
            let len = err.GetBufferSize();
            let bytes = std::slice::from_raw_parts(ptr, len);
            String::from_utf8_lossy(bytes).to_string()
        } else {
            format!("{e}")
        };
        return Err(format!("shader compilation failed: {msg}"));
    }

    blob.ok_or_else(|| "D3DCompile returned null blob".to_string())
}

/// Extract the raw bytecode slice from a compiled shader blob.
///
/// # Safety
/// The returned slice is only valid for the lifetime of the blob.
pub unsafe fn blob_as_slice(blob: &ID3DBlob) -> &[u8] {
    std::slice::from_raw_parts(
        blob.GetBufferPointer() as *const u8,
        blob.GetBufferSize(),
    )
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles with no errors.

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/src/shaders.rs
git commit -m "feat(overlay-dll): add HLSL shader sources and D3DCompile helper"
```

---

### Task 3: D3D11 Pipeline State Backup and Restore

**Files:**
- Modify: `overlay-dll/src/state_backup.rs`

Before rendering our overlay, we must save the game's D3D11 pipeline state so we can restore it after. This prevents visual corruption.

- [ ] **Step 1: Implement state_backup.rs**

```rust
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT;

/// Captured D3D11 pipeline state. Saved before overlay rendering,
/// restored after. Based on the ImGui D3D11 backend's approach.
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
    /// Capture the current pipeline state from the device context.
    ///
    /// # Safety
    /// Must be called from the render thread with a valid context.
    pub unsafe fn save(ctx: &ID3D11DeviceContext) -> Self {
        let mut backup = Self {
            viewports_count: 16,
            scissor_rects_count: 16,
            viewports: [D3D11_VIEWPORT::default(); 16],
            scissor_rects: [RECT::default(); 16],
            rasterizer_state: None,
            blend_state: None,
            blend_factor: [0.0; 4],
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

        ctx.RSGetViewports(&mut backup.viewports_count, Some(backup.viewports.as_mut_ptr()));
        ctx.RSGetScissorRects(
            &mut backup.scissor_rects_count,
            Some(backup.scissor_rects.as_mut_ptr()),
        );
        backup.rasterizer_state = ctx.RSGetState().ok();

        ctx.OMGetBlendState(
            Some(&mut backup.blend_state),
            Some(&mut backup.blend_factor),
            Some(&mut backup.sample_mask),
        );
        ctx.OMGetDepthStencilState(
            Some(&mut backup.depth_stencil_state),
            Some(&mut backup.stencil_ref),
        );

        // Pixel and vertex shaders
        ctx.PSGetShader(&mut backup.ps, None, &mut 0);
        ctx.VSGetShader(&mut backup.vs, None, &mut 0);

        // Input assembler
        backup.primitive_topology = ctx.IAGetPrimitiveTopology();
        backup.input_layout = ctx.IAGetInputLayout().ok();

        // Vertex buffer (slot 0 only)
        let mut vb: [Option<ID3D11Buffer>; 1] = [None];
        let mut stride = 0u32;
        let mut offset = 0u32;
        ctx.IAGetVertexBuffers(0, 1, Some(vb.as_mut_ptr()), Some(&mut stride), Some(&mut offset));
        backup.vertex_buffer = vb[0].take();
        backup.vertex_buffer_stride = stride;
        backup.vertex_buffer_offset = offset;

        // Index buffer
        let mut ib: Option<ID3D11Buffer> = None;
        let mut ib_format = DXGI_FORMAT::default();
        let mut ib_offset = 0u32;
        ctx.IAGetIndexBuffer(Some(&mut ib), Some(&mut ib_format), Some(&mut ib_offset));
        backup.index_buffer = ib;
        backup.index_buffer_format = ib_format;
        backup.index_buffer_offset = ib_offset;

        // Render targets
        let mut rtvs: [Option<ID3D11RenderTargetView>; 1] = [None];
        let mut dsv: Option<ID3D11DepthStencilView> = None;
        ctx.OMGetRenderTargets(Some(&mut rtvs), Some(&mut dsv));
        backup.render_target = rtvs[0].take();
        backup.depth_stencil_view = dsv;

        backup
    }

    /// Restore the previously captured pipeline state.
    ///
    /// # Safety
    /// Must be called from the render thread with the same context.
    pub unsafe fn restore(self, ctx: &ID3D11DeviceContext) {
        ctx.RSSetViewports(Some(&self.viewports[..self.viewports_count as usize]));
        ctx.RSSetScissorRects(Some(&self.scissor_rects[..self.scissor_rects_count as usize]));
        ctx.RSSetState(self.rasterizer_state.as_ref());

        ctx.OMSetBlendState(
            self.blend_state.as_ref(),
            Some(&self.blend_factor),
            self.sample_mask,
        );
        ctx.OMSetDepthStencilState(self.depth_stencil_state.as_ref(), self.stencil_ref);

        ctx.PSSetShader(self.ps.as_ref(), None);
        ctx.VSSetShader(self.vs.as_ref(), None);

        ctx.IASetPrimitiveTopology(self.primitive_topology);
        ctx.IASetInputLayout(self.input_layout.as_ref());

        ctx.IASetVertexBuffers(
            0,
            1,
            Some([self.vertex_buffer].as_ptr()),
            Some(&self.vertex_buffer_stride),
            Some(&self.vertex_buffer_offset),
        );
        ctx.IASetIndexBuffer(
            self.index_buffer.as_ref(),
            self.index_buffer_format,
            self.index_buffer_offset,
        );

        ctx.OMSetRenderTargets(
            Some(&[self.render_target]),
            self.depth_stencil_view.as_ref(),
        );
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles with no errors. Some API signatures may need adjustment — see notes below.

**API notes for windows 0.58:**
- `PSGetShader` / `VSGetShader` take `(&mut Option<T>, Option<&mut [Option<ID3D11ClassInstance>]>, &mut u32)`. Pass `None` for class instances and `&mut 0` for count.
- `IAGetPrimitiveTopology` returns the topology directly (not via out-param) in windows 0.58.
- `IAGetInputLayout` returns `Result<ID3D11InputLayout>`.
- If any of these signatures don't match, fix the call to match the actual windows 0.58 API.

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/src/state_backup.rs
git commit -m "feat(overlay-dll): add D3D11 pipeline state backup and restore"
```

---

### Task 4: Overlay Renderer — Resource Creation and Draw

**Files:**
- Modify: `overlay-dll/src/renderer.rs`

This is the core of Phase 3. The `OverlayRenderer` holds all D3D11 resources and provides `init()` and `render()` methods.

- [ ] **Step 1: Implement renderer.rs**

```rust
use std::ffi::c_void;
use std::mem;

use windows::core::{s, Interface};
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::IDXGISwapChain;

use crate::logging::log_to_file;
use crate::shaders;
use crate::state_backup::D3D11StateBackup;

/// Per-vertex data for the overlay quad.
#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    col: [f32; 4],
}

/// Holds all D3D11 resources needed to render the overlay rectangle.
pub struct OverlayRenderer {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    vertex_shader: ID3D11VertexShader,
    pixel_shader: ID3D11PixelShader,
    input_layout: ID3D11InputLayout,
    vertex_buffer: ID3D11Buffer,
    blend_state: ID3D11BlendState,
    rasterizer_state: ID3D11RasterizerState,
    rtv: Option<ID3D11RenderTargetView>,
}

impl OverlayRenderer {
    /// Create all D3D11 resources from the game's swap chain.
    ///
    /// # Safety
    /// `swap_chain_ptr` must be a valid IDXGISwapChain COM pointer.
    pub unsafe fn init(swap_chain_ptr: *mut c_void) -> Result<Self, String> {
        // Wrap the raw pointer without adding a reference (we don't own it)
        let swap_chain: IDXGISwapChain = mem::transmute_copy(&swap_chain_ptr);
        let swap_chain = mem::ManuallyDrop::new(swap_chain);

        let device: ID3D11Device = swap_chain
            .GetDevice()
            .map_err(|e| format!("GetDevice: {e}"))?;

        let context = device
            .GetImmediateContext()
            .map_err(|e| format!("GetImmediateContext: {e}"))?;

        // Compile shaders
        let vs_blob = shaders::compile_shader(
            shaders::VERTEX_SHADER_HLSL,
            s!("VSMain"),
            s!("vs_4_0"),
        )?;
        let ps_blob = shaders::compile_shader(
            shaders::PIXEL_SHADER_HLSL,
            s!("PSMain"),
            s!("ps_4_0"),
        )?;

        let vs_bytecode = shaders::blob_as_slice(&vs_blob);
        let ps_bytecode = shaders::blob_as_slice(&ps_blob);

        // Create vertex shader
        let mut vertex_shader: Option<ID3D11VertexShader> = None;
        device
            .CreateVertexShader(vs_bytecode, None, Some(&mut vertex_shader))
            .map_err(|e| format!("CreateVertexShader: {e}"))?;
        let vertex_shader = vertex_shader.ok_or("CreateVertexShader returned None")?;

        // Create pixel shader
        let mut pixel_shader: Option<ID3D11PixelShader> = None;
        device
            .CreatePixelShader(ps_bytecode, None, Some(&mut pixel_shader))
            .map_err(|e| format!("CreatePixelShader: {e}"))?;
        let pixel_shader = pixel_shader.ok_or("CreatePixelShader returned None")?;

        // Create input layout
        let layout_desc = [
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
                AlignedByteOffset: 8,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            },
        ];
        let mut input_layout: Option<ID3D11InputLayout> = None;
        device
            .CreateInputLayout(&layout_desc, vs_bytecode, Some(&mut input_layout))
            .map_err(|e| format!("CreateInputLayout: {e}"))?;
        let input_layout = input_layout.ok_or("CreateInputLayout returned None")?;

        // Create vertex buffer — semi-transparent green rectangle in the top-left
        let color = [0.1f32, 0.8, 0.2, 0.6]; // RGBA: green, 60% opacity
        let (l, t, r, b) = (-0.95f32, 0.95, -0.55, 0.75); // clip-space coords
        let vertices: [Vertex; 6] = [
            Vertex { pos: [l, t], col: color },
            Vertex { pos: [r, t], col: color },
            Vertex { pos: [l, b], col: color },
            Vertex { pos: [l, b], col: color },
            Vertex { pos: [r, t], col: color },
            Vertex { pos: [r, b], col: color },
        ];

        let buf_desc = D3D11_BUFFER_DESC {
            ByteWidth: mem::size_of_val(&vertices) as u32,
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_VERTEX_BUFFER.0 as u32,
            ..Default::default()
        };
        let init_data = D3D11_SUBRESOURCE_DATA {
            pSysMem: vertices.as_ptr() as *const _,
            ..Default::default()
        };
        let mut vertex_buffer: Option<ID3D11Buffer> = None;
        device
            .CreateBuffer(&buf_desc, Some(&init_data), Some(&mut vertex_buffer))
            .map_err(|e| format!("CreateBuffer: {e}"))?;
        let vertex_buffer = vertex_buffer.ok_or("CreateBuffer returned None")?;

        // Create blend state (alpha blending)
        let mut blend_desc = D3D11_BLEND_DESC::default();
        blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_SRC_ALPHA,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_ZERO,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        };
        let mut blend_state: Option<ID3D11BlendState> = None;
        device
            .CreateBlendState(&blend_desc, Some(&mut blend_state))
            .map_err(|e| format!("CreateBlendState: {e}"))?;
        let blend_state = blend_state.ok_or("CreateBlendState returned None")?;

        // Create rasterizer state (no culling, solid fill)
        let rast_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let mut rasterizer_state: Option<ID3D11RasterizerState> = None;
        device
            .CreateRasterizerState(&rast_desc, Some(&mut rasterizer_state))
            .map_err(|e| format!("CreateRasterizerState: {e}"))?;
        let rasterizer_state = rasterizer_state.ok_or("CreateRasterizerState returned None")?;

        // Create render target view from back buffer
        let rtv = Self::create_rtv(&device, &swap_chain)?;

        log_to_file("[renderer] initialized successfully");

        Ok(Self {
            device,
            context,
            vertex_shader,
            pixel_shader,
            input_layout,
            vertex_buffer,
            blend_state,
            rasterizer_state,
            rtv: Some(rtv),
        })
    }

    /// Create a render target view from the swap chain's back buffer.
    unsafe fn create_rtv(
        device: &ID3D11Device,
        swap_chain: &IDXGISwapChain,
    ) -> Result<ID3D11RenderTargetView, String> {
        let back_buffer: ID3D11Texture2D = swap_chain
            .GetBuffer(0)
            .map_err(|e| format!("GetBuffer(0): {e}"))?;

        let mut rtv: Option<ID3D11RenderTargetView> = None;
        device
            .CreateRenderTargetView(&back_buffer, None, Some(&mut rtv))
            .map_err(|e| format!("CreateRenderTargetView: {e}"))?;

        rtv.ok_or_else(|| "CreateRenderTargetView returned None".to_string())
    }

    /// Release the render target view. Must be called BEFORE ResizeBuffers.
    pub fn release_rtv(&mut self) {
        self.rtv = None;
    }

    /// Recreate the render target view. Must be called AFTER ResizeBuffers.
    ///
    /// # Safety
    /// `swap_chain_ptr` must be a valid IDXGISwapChain COM pointer.
    pub unsafe fn recreate_rtv(&mut self, swap_chain_ptr: *mut c_void) -> Result<(), String> {
        let swap_chain: IDXGISwapChain = mem::transmute_copy(&swap_chain_ptr);
        let swap_chain = mem::ManuallyDrop::new(swap_chain);
        self.rtv = Some(Self::create_rtv(&self.device, &swap_chain)?);
        Ok(())
    }

    /// Render the overlay rectangle. Called every frame from hooked_present.
    ///
    /// # Safety
    /// Must be called from the game's render thread.
    pub unsafe fn render(&self, swap_chain_ptr: *mut c_void) {
        let rtv = match &self.rtv {
            Some(rtv) => rtv,
            None => return, // RTV released for resize, skip this frame
        };

        let swap_chain: IDXGISwapChain = mem::transmute_copy(&swap_chain_ptr);
        let swap_chain = mem::ManuallyDrop::new(swap_chain);

        // Get swap chain dimensions for viewport
        let mut desc = windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_DESC::default();
        if swap_chain.GetDesc(&mut desc).is_err() {
            return;
        }

        // Save game's pipeline state
        let backup = D3D11StateBackup::save(&self.context);

        // Set our pipeline state
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: desc.BufferDesc.Width as f32,
            Height: desc.BufferDesc.Height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        self.context.RSSetViewports(Some(&[viewport]));
        self.context.RSSetState(Some(&self.rasterizer_state));

        self.context
            .OMSetRenderTargets(Some(&[Some(rtv.clone())]), None);
        self.context
            .OMSetBlendState(Some(&self.blend_state), Some(&[0.0; 4]), 0xFFFFFFFF);

        self.context.IASetInputLayout(Some(&self.input_layout));
        self.context
            .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);

        let stride = mem::size_of::<Vertex>() as u32;
        let offset = 0u32;
        self.context.IASetVertexBuffers(
            0,
            1,
            Some([Some(self.vertex_buffer.clone())].as_ptr()),
            Some(&stride),
            Some(&offset),
        );

        self.context
            .VSSetShader(Some(&self.vertex_shader), None);
        self.context
            .PSSetShader(Some(&self.pixel_shader), None);

        // Draw the quad (6 vertices = 2 triangles)
        self.context.Draw(6, 0);

        // Restore game's pipeline state
        backup.restore(&self.context);
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles. There may be API signature mismatches with windows 0.58 — fix iteratively. Key areas to watch:
- `BindFlags` may need `D3D11_BIND_VERTEX_BUFFER.0 as u32` or just the raw integer.
- `BlendEnable` and `DepthClipEnable` may need `BOOL(1)` instead of `true.into()`.
- `GetImmediateContext` may return `Result<ID3D11DeviceContext>` or take an out-param — check the error.

- [ ] **Step 3: Commit**

```bash
git add overlay-dll/src/renderer.rs
git commit -m "feat(overlay-dll): implement OverlayRenderer with shaders, blend state, and quad draw"
```

---

### Task 5: Wire Renderer into Present and ResizeBuffers Hooks

**Files:**
- Modify: `overlay-dll/src/present.rs`

Update `hooked_present` to lazy-initialize the `OverlayRenderer` on first call and render every frame. Update `hooked_resize_buffers` to release/recreate the RTV.

- [ ] **Step 1: Update present.rs**

Replace the entire contents of `overlay-dll/src/present.rs` with:

```rust
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use windows::core::HRESULT;

use crate::logging::log_to_file;
use crate::renderer::OverlayRenderer;

pub type PresentFn = unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT;
pub type Present1Fn = unsafe extern "system" fn(*mut c_void, u32, u32, *const c_void) -> HRESULT;
pub type ResizeBuffersFn = unsafe extern "system" fn(*mut c_void, u32, u32, u32, u32, u32) -> HRESULT;

pub static mut ORIGINAL_PRESENT: Option<PresentFn> = None;
pub static mut ORIGINAL_PRESENT1: Option<Present1Fn> = None;
pub static mut ORIGINAL_RESIZE_BUFFERS: Option<ResizeBuffersFn> = None;

static FRAME_COUNT: AtomicU64 = AtomicU64::new(0);

/// Global renderer, initialized on first Present call.
/// Using a raw pointer behind UnsafeCell pattern because OnceLock doesn't support
/// mutable access and the renderer needs &mut self for release_rtv/recreate_rtv.
static RENDERER_INIT: OnceLock<()> = OnceLock::new();
static mut RENDERER: Option<OverlayRenderer> = None;

/// Initialize the renderer on the first Present call.
unsafe fn ensure_renderer(swap_chain: *mut c_void) {
    RENDERER_INIT.get_or_init(|| {
        match OverlayRenderer::init(swap_chain) {
            Ok(r) => {
                RENDERER = Some(r);
                log_to_file("[present] renderer initialized on first frame");
            }
            Err(e) => {
                log_to_file(&format!("[present] FATAL: renderer init failed: {e}"));
            }
        }
    });
}

/// Common rendering logic shared by hooked_present and hooked_present1.
unsafe fn render_overlay(swap_chain: *mut c_void) {
    ensure_renderer(swap_chain);
    if let Some(renderer) = &RENDERER {
        renderer.render(swap_chain);
    }
}

pub unsafe extern "system" fn hooked_present(
    swap_chain: *mut c_void,
    sync_interval: u32,
    flags: u32,
) -> HRESULT {
    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
    if count % 300 == 0 {
        log_to_file(&format!(
            "[present] frame {count}, sync_interval={sync_interval}, flags={flags:#010x}"
        ));
    }

    render_overlay(swap_chain);

    if let Some(original) = ORIGINAL_PRESENT {
        original(swap_chain, sync_interval, flags)
    } else {
        HRESULT(0)
    }
}

pub unsafe extern "system" fn hooked_present1(
    swap_chain: *mut c_void,
    sync_interval: u32,
    present_flags: u32,
    present_params: *const c_void,
) -> HRESULT {
    let count = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
    if count % 300 == 0 {
        log_to_file(&format!(
            "[present1] frame {count}, sync_interval={sync_interval}, flags={present_flags:#010x}"
        ));
    }

    render_overlay(swap_chain);

    if let Some(original) = ORIGINAL_PRESENT1 {
        original(swap_chain, sync_interval, present_flags, present_params)
    } else {
        HRESULT(0)
    }
}

pub unsafe extern "system" fn hooked_resize_buffers(
    swap_chain: *mut c_void,
    buffer_count: u32,
    width: u32,
    height: u32,
    new_format: u32,
    swap_chain_flags: u32,
) -> HRESULT {
    log_to_file(&format!(
        "[resize_buffers] {width}x{height}, buffers={buffer_count}"
    ));

    // Release RTV before resize (holding a reference to the back buffer blocks resize)
    if let Some(renderer) = &mut RENDERER {
        renderer.release_rtv();
    }

    // Call original ResizeBuffers
    let result = if let Some(original) = ORIGINAL_RESIZE_BUFFERS {
        original(swap_chain, buffer_count, width, height, new_format, swap_chain_flags)
    } else {
        HRESULT(0)
    };

    // Recreate RTV after resize
    if result.is_ok() {
        if let Some(renderer) = &mut RENDERER {
            if let Err(e) = renderer.recreate_rtv(swap_chain) {
                log_to_file(&format!("[resize_buffers] failed to recreate RTV: {e}"));
            }
        }
    }

    result
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p omni-overlay-dll`
Expected: Compiles with no errors.

- [ ] **Step 3: Run workspace tests**

Run: `cargo test --workspace`
Expected: All existing tests pass.

- [ ] **Step 4: Commit**

```bash
git add overlay-dll/src/present.rs
git commit -m "feat(overlay-dll): wire OverlayRenderer into Present and ResizeBuffers hooks"
```

---

### Task 6: Build Release and Integration Test

This is a manual test — no code changes.

- [ ] **Step 1: Build release DLL**

Run: `cargo build -p omni-overlay-dll --release`
Expected: Compiles. Produces `target/release/omni_overlay_dll.dll`.

- [ ] **Step 2: Clear old log and inject**

```powershell
Remove-Item $env:TEMP\omni_overlay.log -ErrorAction SilentlyContinue

# Launch a DX11 game, find PID
Get-Process | Where-Object { $_.MainWindowTitle -ne "" } | Select-Object Id, ProcessName, MainWindowTitle

# Inject
cargo run -p omni-host -- <GAME_PID> "C:\Users\DyllenOwens\Projects\omni\target\release\omni_overlay_dll.dll"
```

- [ ] **Step 3: Verify log output**

```powershell
Get-Content $env:TEMP\omni_overlay.log
```

Expected log lines (in addition to hook messages):
```
[...] [renderer] initialized successfully
[...] [present] renderer initialized on first frame
[...] [present] frame 0, sync_interval=0, flags=...
[...] [present] frame 300, ...
```

- [ ] **Step 4: Visually confirm the rectangle**

Look at the top-left corner of the game window. You should see a **semi-transparent green rectangle** (approximately 20% of the screen width, 10% height) overlaid on the game.

If the rectangle is NOT visible:
- Check the log for any `FATAL` or error messages
- The rectangle uses clip-space coords (-0.95 to -0.55 horizontal, 0.75 to 0.95 vertical) which maps to the top-left ~20% x 10% of the screen
- If the game renders in exclusive fullscreen, try windowed or borderless mode
- The rectangle is 60% opaque green — it should be visible over any game content

- [ ] **Step 5: Test window resize**

If the game supports windowed mode, resize the window. The log should show:
```
[...] [resize_buffers] 1280x720, buffers=2
```

The rectangle should remain visible after the resize (the RTV is recreated).

- [ ] **Step 6: Commit any fixes**

```bash
git add -A
git commit -m "fix: address issues found during overlay rectangle integration test"
```

---

## Phase 3 Complete — Summary

At this point you have:

1. A visible overlay rendered on top of a DX11 game
2. Semi-transparent green rectangle drawn with proper alpha blending
3. Pipeline state saved/restored so the game's rendering is not corrupted
4. Render target view properly released/recreated on window resize
5. Lazy initialization — renderer is created on first Present call

**Next:** Phase 4 will replace the hardcoded rectangle with text rendering (using a font atlas) and read widget data from the shared memory region to display real sensor values.
