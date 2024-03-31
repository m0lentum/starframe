use std::{borrow::Cow, mem::size_of};
use zerocopy::{AsBytes, FromBytes};

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
    pub fn shade(self) -> PostProcessContext<'a> {
        let device = super::Renderer::device();
        let queue = super::Renderer::queue();
        queue.submit(Some(self.encoder.finish()));

        self.renderer.begin_frame();
        let mut pp_ctx = PostProcessContext {
            renderer: self.renderer,
            encoder: Some(
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default()),
            ),
            is_first_draw: true,
        };

        // run the shading

        {
            let mut shade_pass = pp_ctx.pass();
            // TODO
        }

        pp_ctx
    }
}

pub struct DeferredPass<'a> {
    pub pass: wgpu::RenderPass<'a>,
    pub target_size: (u32, u32),
}

pub struct PostProcessContext<'a> {
    renderer: &'a mut super::Renderer,
    // encoder in an Option so that we can take it out
    // and submit it on drop without unsafe
    encoder: Option<wgpu::CommandEncoder>,
    is_first_draw: bool,
}

impl<'a> PostProcessContext<'a> {
    /// Begin a render pass that draws on top of what's already in the window.
    pub fn pass(&mut self) -> wgpu::RenderPass {
        // encoder always exists, it's only removed on drop
        let encoder = self.encoder.as_mut().unwrap();
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("deferred"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                // frame was begun in DeferredContext::submit,
                // which is the only way to create a PostProcessContext
                view: &self.renderer.active_frame.as_ref().unwrap().view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: if self.is_first_draw {
                        wgpu::LoadOp::Clear(wgpu::Color::BLACK)
                    } else {
                        wgpu::LoadOp::Load
                    },
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.renderer.gbufs.depth,
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

        pass
    }
}

//
// shading pipeline
//

pub struct ShadingPipeline {
    pipeline: wgpu::RenderPipeline,
    gbufs_bind_group_layout: wgpu::BindGroupLayout,
    // bind group must be recreated when the window size changes
    pub(super) gbufs_bind_group: wgpu::BindGroup,
    light_buf: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct LightUniforms {
    direct_color: [f32; 3],
    _pad0: u32,
    ambient_color: [f32; 3],
    _pad1: u32,
    direction: [f32; 3],
    _pad2: u32,
}

impl From<crate::DirectionalLight> for LightUniforms {
    fn from(l: crate::DirectionalLight) -> Self {
        Self {
            direct_color: l.direct_color,
            _pad0: 0,
            ambient_color: l.ambient_color,
            _pad1: 0,
            direction: l.direction.normalized().into(),
            _pad2: 0,
        }
    }
}

impl ShadingPipeline {
    pub fn new(gbufs: &GBuffers) -> Self {
        let device = super::Renderer::device();

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("deferred shading"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "../shaders/deferred_shade.wgsl"
            ))),
        });

        // gbuffer bind group

        let tex_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let gbufs_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("deferred shading gbuffers"),
                entries: &[
                    tex_entry(0),
                    tex_entry(1),
                    tex_entry(2),
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                        count: None,
                    },
                ],
            });
        let gbufs_bind_group = Self::create_gbufs_bind_group(&gbufs_bind_group_layout, gbufs);

        // light

        let light_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<LightUniforms>() as _,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh lights"),
            mapped_at_creation: false,
        });

        let light_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<LightUniforms>() as _),
                    },
                    count: None,
                }],
                label: Some("deferred lights"),
            });

        let light_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deferred lights"),
            layout: &light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buf.as_entire_binding(),
            }],
        });

        // pipeline

        let pl_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("deferred shading"),
            bind_group_layouts: &[&gbufs_bind_group_layout, &light_bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("deferred shading"),
            layout: Some(&pl_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(super::SWAPCHAIN_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        Self {
            pipeline,
            gbufs_bind_group_layout,
            gbufs_bind_group,
            light_buf,
            light_bind_group,
        }
    }

    fn create_gbufs_bind_group(
        layout: &wgpu::BindGroupLayout,
        gbufs: &GBuffers,
    ) -> wgpu::BindGroup {
        let device = super::Renderer::device();
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deferred shading gbuffers"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&gbufs.position.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&gbufs.normal.view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&gbufs.albedo.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&gbufs.sampler),
                },
            ],
        })
    }

    pub(super) fn update_gbufs_bind_group(&mut self, gbufs: &GBuffers) {
        self.gbufs_bind_group = Self::create_gbufs_bind_group(&self.gbufs_bind_group_layout, gbufs);
    }
}
