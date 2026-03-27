use windows::Win32::Graphics::Direct3D::Fxc::D3DCompile;
use windows::Win32::Graphics::Direct3D::ID3DBlob;
use windows::core::PCSTR;

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
pub unsafe fn blob_as_slice(blob: &ID3DBlob) -> &[u8] {
    std::slice::from_raw_parts(
        blob.GetBufferPointer() as *const u8,
        blob.GetBufferSize(),
    )
}
