//
// GBuffer types
//

/// All GBuffers needed for Starframe's deferred shading pipeline.
pub struct GBuffers {
    pub dimensions: (u32, u32),
    // depth is not a GBuffer because it requires different multisampling configuration
    pub depth_tex: wgpu::Texture,
    pub depth: wgpu::TextureView,
    pub position: GBuffer,
    pub normal: GBuffer,
    pub albedo: GBuffer,
    pub sampler: wgpu::Sampler,
}

impl GBuffers {
    pub fn new(dimensions: (u32, u32), sample_count: u32) -> Self {
        let device = super::Renderer::device();

        let depth_tex = create_texture(
            dimensions,
            sample_count,
            wgpu::TextureFormat::Depth16Unorm,
            Some("depth"),
        );
        let depth = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());

        // non-filtering sampler,
        // none needed because we're sampling and writing screen-sized textures
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gbuffer sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

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
            sampler,
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

pub(super) fn create_texture(
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

//
// context / pass
//

pub struct DeferredContext<'a> {
    renderer: &'a mut super::Renderer,
    encoder: wgpu::CommandEncoder,
    is_first_draw: bool,
}

impl<'a> DeferredContext<'a> {
    pub fn new(renderer: &'a mut super::Renderer) -> Self {
        let device = super::Renderer::device();
        let encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("deferred"),
        });
        Self {
            renderer,
            encoder,
            is_first_draw: true,
        }
    }

    pub fn pass(&mut self) -> DeferredPass<'_> {
        // encoder always exists, it's only removed on drop
        let gbufs = &self.renderer.gbufs;
        let pass = self.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("deferred"),
            color_attachments: &[
                Some(gbufs.position.color_attachment(self.is_first_draw)),
                Some(gbufs.normal.color_attachment(self.is_first_draw)),
                Some(gbufs.albedo.color_attachment(self.is_first_draw)),
            ],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &gbufs.depth,
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

        self.is_first_draw = false;

        DeferredPass {
            pass,
            target_size: gbufs.dimensions,
        }
    }

    /// Shade the image drawn with deferred rendering
    /// and move on to rendering directly to the window.
    pub fn shade(self, light: crate::DirectionalLight) -> PostShadeContext<'a> {
        let device = super::Renderer::device();
        let queue = super::Renderer::queue();
        queue.submit(Some(self.encoder.finish()));

        self.renderer.begin_frame();

        // run the shading

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let mut shade_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shade"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                // frame was just begun so this must exist
                view: &self.renderer.active_frame.as_ref().unwrap().view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        self.renderer
            .deferred_shading_pl
            .draw(&mut shade_pass, light);
        drop(shade_pass);

        PostShadeContext {
            renderer: self.renderer,
            encoder: Some(encoder),
        }
    }
}

pub struct DeferredPass<'a> {
    pub pass: wgpu::RenderPass<'a>,
    pub target_size: (u32, u32),
}

pub struct PostShadeContext<'a> {
    renderer: &'a mut super::Renderer,
    // encoder in an Option so that we can take it out
    // and submit it on drop without unsafe
    encoder: Option<wgpu::CommandEncoder>,
}

impl<'a> PostShadeContext<'a> {
    /// Begin a render pass that draws on top of what's already in the window.
    pub fn pass(&mut self) -> wgpu::RenderPass {
        // encoder always exists, it's only removed on drop
        let encoder = self.encoder.as_mut().unwrap();
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("post-shade"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.renderer.msaa_view,
                resolve_target: self.renderer.active_frame.as_ref().map(|f| &f.view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.renderer.gbufs.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        })
    }

    #[inline]
    pub fn encoder_mut(&mut self) -> &mut wgpu::CommandEncoder {
        self.encoder.as_mut().unwrap()
    }
}

impl<'a> Drop for PostShadeContext<'a> {
    // automatically submit on drop
    fn drop(&mut self) {
        let queue = super::Renderer::queue();
        queue.submit(self.encoder.take().map(|enc| enc.finish()));
    }
}
