pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth16Unorm;

/// All GBuffers needed for Starframe's deferred shading pipeline.
pub struct GBuffers {
    pub depth: GBuffer,
    pub position: GBuffer,
    pub normal: GBuffer,
    pub albedo: GBuffer,
}

impl GBuffers {
    pub fn new(dimensions: (u32, u32), sample_count: u32) -> Self {
        let gbuf = |format: wgpu::TextureFormat, label: &str| {
            GBuffer::new(dimensions, sample_count, format, Some(label))
        };
        Self {
            depth: gbuf(DEPTH_FORMAT, "depth"),
            position: gbuf(wgpu::TextureFormat::Rgba8Unorm, "position"),
            normal: gbuf(wgpu::TextureFormat::Rgba8Unorm, "normal"),
            albedo: gbuf(wgpu::TextureFormat::Rgba8Unorm, "albedo"),
        }
    }
}

/// A fullscreen texture that can be rendered to.
pub struct GBuffer {
    pub view: wgpu::TextureView,
}

impl GBuffer {
    pub fn new(
        dimensions: (u32, u32),
        sample_count: u32,
        format: wgpu::TextureFormat,
        label: Option<&str>,
    ) -> Self {
        let device = super::Renderer::device();
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
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self { view }
    }
}
