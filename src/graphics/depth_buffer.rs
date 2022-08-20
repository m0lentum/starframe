use super::outlines;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

/// Default depth buffer used by the Starframe renderer.
/// Includes a stencil used to draw outlines.
pub struct DepthBuffer {
    pub view: wgpu::TextureView,
}

impl DepthBuffer {
    pub fn new(
        device: &wgpu::Device,
        dimensions: (u32, u32),
        sample_count: u32,
        label: Option<&'static str>,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label,
            size: wgpu::Extent3d {
                width: dimensions.0,
                height: dimensions.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self { view }
    }

    pub fn default_depth_stencil_state() -> wgpu::DepthStencilState {
        wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState {
                front: outlines::WRITE_STENCIL,
                back: outlines::WRITE_STENCIL,
                read_mask: 0xff,
                write_mask: 0xff,
            },
            bias: wgpu::DepthBiasState::default(),
        }
    }
}
