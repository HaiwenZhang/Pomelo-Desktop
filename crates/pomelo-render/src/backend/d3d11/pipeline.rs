//! Audited FFI boundary. All COM objects belong to the GPUI-provided device.

use super::{ProbeScene, Uniforms, geometry};
use anyhow::Context as _;
use gpui::NativeGpuContext;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D::{
                D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
                Fxc::D3DCompile, ID3DBlob, ID3DInclude,
            },
            Direct3D11::*,
        },
    },
    core::{PCSTR, s},
};

pub(super) struct Pipeline {
    vertex: ID3D11VertexShader,
    fragment: ID3D11PixelShader,
    uniforms: ID3D11Buffer,
    blend: ID3D11BlendState,
    rasterizer: ID3D11RasterizerState,
}

fn compile(entry: PCSTR, target: PCSTR) -> anyhow::Result<ID3DBlob> {
    compile_shader(
        include_str!("../../shaders/pcb.hlsl"),
        s!("pomelo-render/pcb.hlsl"),
        entry,
        target,
    )
}

pub(super) fn compile_shader(
    source: &str,
    name: PCSTR,
    entry: PCSTR,
    target: PCSTR,
) -> anyhow::Result<ID3DBlob> {
    let source = crate::scene::colors::hlsl_library() + source;
    let mut code = None;
    let mut errors = None;
    // SAFETY: the owned UTF-8 source and out-pointers live for the synchronous compiler call.
    let result = unsafe {
        D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            name,
            None,
            None::<&ID3DInclude>,
            entry,
            target,
            0,
            0,
            &mut code,
            Some(&mut errors),
        )
    };
    if let Err(error) = result {
        let details = errors
            .map(|blob| {
                // SAFETY: D3DBlob owns this byte range until the conversion completes.
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        blob.GetBufferPointer().cast::<u8>(),
                        blob.GetBufferSize(),
                    )
                };
                String::from_utf8_lossy(bytes)
                    .trim_end_matches('\0')
                    .to_owned()
            })
            .unwrap_or_default();
        return Err(anyhow::Error::new(error).context(format!("GPU_SHADER_COMPILE: {details}")));
    }
    code.context("GPU_SHADER_BLOB_MISSING")
}

impl Pipeline {
    pub(super) fn new(device: &ID3D11Device) -> anyhow::Result<Self> {
        let vertex_blob = compile(s!("pcb_vertex"), s!("vs_4_1"))?;
        let fragment_blob = compile(s!("pcb_fragment"), s!("ps_4_1"))?;
        let (mut vertex, mut fragment, mut uniforms, mut blend, mut rasterizer) =
            (None, None, None, None, None);
        // SAFETY: shader blobs remain alive, descriptors are initialized, and every output is checked.
        unsafe {
            device.CreateVertexShader(
                std::slice::from_raw_parts(
                    vertex_blob.GetBufferPointer().cast(),
                    vertex_blob.GetBufferSize(),
                ),
                None,
                Some(&mut vertex),
            )?;
            device.CreatePixelShader(
                std::slice::from_raw_parts(
                    fragment_blob.GetBufferPointer().cast(),
                    fragment_blob.GetBufferSize(),
                ),
                None,
                Some(&mut fragment),
            )?;
            device.CreateBuffer(
                &D3D11_BUFFER_DESC {
                    ByteWidth: std::mem::size_of::<Uniforms>() as u32,
                    Usage: D3D11_USAGE_DYNAMIC,
                    BindFlags: D3D11_BIND_CONSTANT_BUFFER.0 as u32,
                    CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                    ..Default::default()
                },
                None,
                Some(&mut uniforms),
            )?;
            let mut blend_desc = D3D11_BLEND_DESC::default();
            blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: true.into(),
                SrcBlend: D3D11_BLEND_SRC_ALPHA,
                DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            };
            device.CreateBlendState(&blend_desc, Some(&mut blend))?;
            device.CreateRasterizerState(
                &D3D11_RASTERIZER_DESC {
                    FillMode: D3D11_FILL_SOLID,
                    CullMode: D3D11_CULL_NONE,
                    DepthClipEnable: true.into(),
                    ScissorEnable: true.into(),
                    ..Default::default()
                },
                Some(&mut rasterizer),
            )?;
        }
        Ok(Self {
            vertex: vertex.context("GPU_VERTEX_SHADER_MISSING")?,
            fragment: fragment.context("GPU_PIXEL_SHADER_MISSING")?,
            uniforms: uniforms.context("GPU_UNIFORM_BUFFER_MISSING")?,
            blend: blend.context("GPU_BLEND_STATE_MISSING")?,
            rasterizer: rasterizer.context("GPU_RASTERIZER_MISSING")?,
        })
    }

    pub(super) fn draw(
        &self,
        frame: &NativeGpuContext<'_>,
        scene: &ProbeScene,
    ) -> anyhow::Result<u64> {
        let context = frame.context;
        let shapes = geometry(frame, scene);
        let clip = frame.bounds.intersect(&frame.content_mask.bounds);
        let rect = RECT {
            left: clip.origin.x.0.floor().max(0.0) as i32,
            top: clip.origin.y.0.floor().max(0.0) as i32,
            right: (clip.origin.x.0 + clip.size.width.0)
                .ceil()
                .min(frame.viewport[0]) as i32,
            bottom: (clip.origin.y.0 + clip.size.height.0)
                .ceil()
                .min(frame.viewport[1]) as i32,
        };
        // SAFETY: invoked synchronously by GPUI on the owning immediate context. All resources
        // use its current device; reset drops them before device replacement. GPUI restores state.
        unsafe {
            context.IASetInputLayout(None);
            context.IASetPrimitiveTopology(if scene.triangle {
                D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST
            } else {
                D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP
            });
            context.VSSetShader(&self.vertex, None);
            context.PSSetShader(&self.fragment, None);
            context.RSSetState(&self.rasterizer);
            context.RSSetScissorRects(Some(&[rect]));
            context.OMSetBlendState(&self.blend, None, u32::MAX);
            context.VSSetConstantBuffers(0, Some(&[Some(self.uniforms.clone())]));
            context.PSSetConstantBuffers(0, Some(&[Some(self.uniforms.clone())]));
            for data in &shapes {
                let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                context.Map(
                    &self.uniforms,
                    0,
                    D3D11_MAP_WRITE_DISCARD,
                    0,
                    Some(&mut mapped),
                )?;
                // Uniforms has an asserted, padding-free 112-byte ABI; the allocation has that size.
                std::ptr::copy_nonoverlapping(
                    (data as *const Uniforms).cast::<u8>(),
                    mapped.pData.cast::<u8>(),
                    std::mem::size_of::<Uniforms>(),
                );
                context.Unmap(&self.uniforms, 0);
                context.Draw(if scene.triangle { 3 } else { 4 }, 0);
            }
        }
        Ok(shapes.len() as u64)
    }
}
