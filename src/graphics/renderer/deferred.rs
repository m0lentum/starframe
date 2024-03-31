/// All GBuffers needed for Starframe's deferred shading pipeline.
pub struct GBuffers {
    pub dimensions: (u32, u32),
    // depth is not a GBuffer because it requires different multisampling configuration
    pub depth_tex: wgpu::Texture,
    pub depth: wgpu::TextureView,
    pub position: GBuffer,
    pub normal: GBuffer,
    pub albedo: GBuffer,
}

impl GBuffers {
    pub fn new(dimensions: (u32, u32), sample_count: u32) -> Self {
        let depth_tex = create_texture(
            dimensions,
            sample_count,
            wgpu::TextureFormat::Depth16Unorm,
            Some("depth"),
        );
        let depth = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        let gbuf = |format: wgpu::TextureFormat, label: &str| {
            GBuffer::new(dimensions, sample_count, format, Some(label))
        };
        Self {
            dimensions,
            depth_tex,
            depth,
            position: gbuf(wgpu::TextureFormat::Rgba16Float, "position"),
            normal: gbuf(wgpu::TextureFormat::Rgba16Float, "normal"),
            albedo: gbuf(wgpu::TextureFormat::Rgba8Unorm, "albedo"),
        }
    }
}

/// A fullscreen texture that can be rendered to.
pub struct GBuffer {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub msaa_view: wgpu::TextureView,
}

impl GBuffer {
    pub fn new(
        dimensions: (u32, u32),
        sample_count: u32,
        format: wgpu::TextureFormat,
        label: Option<&str>,
    ) -> Self {
        let texture = create_texture(dimensions, 1, format, label);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let msaa_texture = create_texture(dimensions, sample_count, format, label);
        let msaa_view = msaa_texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            texture,
            view,
            msaa_view,
        }
    }

    fn color_attachment<'s, 't: 's>(
        &'s self,
        is_first_draw: bool,
    ) -> wgpu::RenderPassColorAttachment<'s> {
        wgpu::RenderPassColorAttachment {
            view: &self.msaa_view,
            resolve_target: Some(&self.view),
            ops: wgpu::Operations {
                load: if is_first_draw {
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                } else {
                    wgpu::LoadOp::Load
                },
                store: wgpu::StoreOp::Store,
            },
        }
    }
}

fn create_texture(
    dimensions: (u32, u32),
    sample_count: u32,
    format: wgpu::TextureFormat,
    label: Option<&str>,
) -> wgpu::Texture {
    let device = super::Renderer::device();

    device.create_texture(&wgpu::TextureDescriptor {
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
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

pub struct DeferredContext<'a> {
    gbufs: &'a GBuffers,
    // encoder in an Option so that we can take it out
    // and submit it on drop without unsafe
    encoder: Option<wgpu::CommandEncoder>,
    is_first_draw: bool,
}

impl<'a> DeferredContext<'a> {
    pub fn new(gbufs: &'a GBuffers) -> Self {
        let device = super::Renderer::device();
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("deferred"),
        });
        Self {
            gbufs,
            encoder: Some(encoder),
            is_first_draw: true,
        }
    }

    pub fn draw<'pass, 's: 'pass, Draw: FnOnce(&mut DeferredPass<'pass>)>(
        &'s mut self,
        draw: Draw,
    ) {
        let mut pass = self.pass();
        draw(&mut pass);
    }

    fn pass(&mut self) -> DeferredPass<'_> {
        // encoder always exists, it's only removed on drop
        let encoder = self.encoder.as_mut().unwrap();
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("deferred"),
            color_attachments: &[
                Some(self.gbufs.position.color_attachment(self.is_first_draw)),
                Some(self.gbufs.normal.color_attachment(self.is_first_draw)),
                Some(self.gbufs.albedo.color_attachment(self.is_first_draw)),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.gbufs.depth,
                depth_ops: Some(wgpu::Operations {
                    load: if self.is_first_draw {
                        wgpu::LoadOp::Clear(0.)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        DeferredPass {
            pass,
            target_size: self.gbufs.dimensions,
        }
    }
}

impl<'a> Drop for DeferredContext<'a> {
    fn drop(&mut self) {
        let Some(encoder) = self.encoder.take() else {
            return;
        };
        let queue = super::Renderer::queue();
        queue.submit(Some(encoder.finish()));
    }
}

pub struct DeferredPass<'a> {
    pub pass: wgpu::RenderPass<'a>,
    pub target_size: (u32, u32),
}
