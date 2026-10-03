//! Atlas textures on the GPUI D3D11 device. No standalone GPU runtime.
use crate::text::msdf::MsdfFont;
use anyhow::Context as _;
use std::collections::BTreeMap;
use windows::Win32::Graphics::{
    Direct3D11::*,
    Dxgi::Common::{DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SAMPLE_DESC},
};

pub(super) struct AtlasResources {
    pub pages: BTreeMap<u16, ID3D11ShaderResourceView>,
    pub sampler: ID3D11SamplerState,
}
impl AtlasResources {
    pub fn new(device: &ID3D11Device, font: &MsdfFont) -> anyhow::Result<Self> {
        let mut pages = BTreeMap::new();
        for page in &font.pages {
            let (mut texture, mut resource) = (None, None);
            // SAFETY: a validated decoded image remains alive during immutable texture creation;
            // output handles are checked. UNORM deliberately avoids sRGB conversion of distance data.
            unsafe {
                device.CreateTexture2D(
                    &D3D11_TEXTURE2D_DESC {
                        Width: page.width,
                        Height: page.height,
                        MipLevels: 1,
                        ArraySize: 1,
                        Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                        SampleDesc: DXGI_SAMPLE_DESC {
                            Count: 1,
                            Quality: 0,
                        },
                        Usage: D3D11_USAGE_IMMUTABLE,
                        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
                        ..Default::default()
                    },
                    Some(&D3D11_SUBRESOURCE_DATA {
                        pSysMem: page.rgba.as_ptr().cast(),
                        SysMemPitch: page.width * 4,
                        ..Default::default()
                    }),
                    Some(&mut texture),
                )?;
                device.CreateShaderResourceView(
                    texture.as_ref().context("GPU_MSDF_TEXTURE_MISSING")?,
                    None,
                    Some(&mut resource),
                )?;
            }
            pages.insert(page.page, resource.context("GPU_MSDF_RESOURCE_MISSING")?);
        }
        let mut sampler = None;
        // SAFETY: initialized sampler descriptor, valid device and checked output.
        unsafe {
            device.CreateSamplerState(
                &D3D11_SAMPLER_DESC {
                    Filter: D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT,
                    AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
                    AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    MaxLOD: 0.0,
                    ..Default::default()
                },
                Some(&mut sampler),
            )?;
        }
        Ok(Self {
            pages,
            sampler: sampler.context("GPU_MSDF_SAMPLER_MISSING")?,
        })
    }
}
