//! Utilities for drawing outlines around things.

use std::borrow::Cow;
use zerocopy::{AsBytes, FromBytes};

//
// parameters
//

// stencil test for renderers that want to support outlines to use.
// always writes if a pixel is drawn, and writes an "enum" value
// that determines whether or not outlines are drawn for that pixel
pub(crate) const WRITE_STENCIL: wgpu::StencilFaceState = wgpu::StencilFaceState {
    compare: wgpu::CompareFunction::Always,
    fail_op: wgpu::StencilOperation::Keep,
    depth_fail_op: wgpu::StencilOperation::Keep,
    pass_op: wgpu::StencilOperation::Replace,
};

const GBUF_COUNT: usize = 3;

/// Renderer that draws outlines for things drawn earlier that support it.
pub struct OutlineRenderer {
    pub params: OutlineParams,
    // two screen-size textures to alternate between for jump flood passes
    // plus one to store previous frame's result for temporal smoothing
    gbufs: [GBuffer; GBUF_COUNT],
    gbuf_bind_group_layout: wgpu::BindGroupLayout,
    gbuf_size: (u32, u32),
    // notifications for window size changes
    swapchain_gen: usize,

    init_step: InitStep,
    dist_step: DistanceStep,
    draw_step: DrawStep,

    // which buffer we wrote to last frame, to figure out where to draw next
    // without overwriting history we need for smoothing
    last_gbuf_idx: usize,
}

/// Parameters to configure outline rendering.
#[derive(Clone, Copy, Debug)]
pub struct OutlineParams {
    /// Thickness measured in pixels.
    pub thickness: u32,
    pub color: [f32; 4],
    /// The shape of the outline around corners.
    pub shape: OutlineShape,
}
impl Default for OutlineParams {
    fn default() -> Self {
        Self {
            thickness: 10,
            color: [0.0, 0.0, 0.0, 1.0],
            shape: Default::default(),
        }
    }
}

/// The shape of outlines around corners and curves,
/// defined as a weighted average of three different norm functions.
#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
pub struct OutlineShape {
    /// Weight of the l1 norm (a.k.a. manhattan distance).
    /// Produces a rhombus shape.
    pub l1: f32,
    /// Weight of the l2 norm (a.k.a. euclidean distance).
    /// Produces a circular shape.
    pub l2: f32,
    /// Weight of the infinity norm (a.k.a. maximum norm).
    /// Produces a square shape.
    pub inf: f32,
}
impl OutlineShape {
    #[inline]
    pub fn new(l1: f32, l2: f32, inf: f32) -> Self {
        Self { l1, l2, inf }
    }

    #[inline]
    pub fn circle() -> Self {
        Self::new(0.0, 1.0, 0.0)
    }

    #[inline]
    pub fn octagon() -> Self {
        Self::new(1.0 / 3.0, 0.0, 2.0 / 3.0)
    }

    #[inline]
    pub fn rhombus() -> Self {
        Self::new(1.0, 0.0, 0.0)
    }

    #[inline]
    pub fn square() -> Self {
        Self::new(0.0, 0.0, 1.0)
    }

    #[inline]
    pub fn rounded_square() -> Self {
        Self::new(0.0, 1.0 / 2.0, 1.0 / 2.0)
    }

    /// Linearly interpolate between this and another shape.
    #[inline]
    pub fn lerp(&self, t: f32, other: Self) -> Self {
        let t_ = 1.0 - t;
        Self {
            l1: t_ * self.l1 + t * other.l1,
            l2: t_ * self.l2 + t * other.l2,
            inf: t_ * self.inf + t * other.inf,
        }
    }

    #[inline]
    fn normalized(&self) -> Self {
        let sum = self.l1 + self.l2 + self.inf;
        Self {
            l1: self.l1 / sum,
            l2: self.l2 / sum,
            inf: self.inf / sum,
        }
    }
}
impl Default for OutlineShape {
    fn default() -> Self {
        Self::octagon()
    }
}

//
// internals
//

/// The initialization step of JFA, drawing seed fragment positions into a texture.
struct InitStep {
    pipeline: wgpu::RenderPipeline,
    stencil_bind_group_layout: wgpu::BindGroupLayout,
    stencil_bind_group: wgpu::BindGroup,
}

/// The actual jump flooding pass generating a distance field.
struct DistanceStep {
    pipeline: wgpu::RenderPipeline,
    uniform_bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
}
#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct DistanceUniforms {
    step_size: u32,
    shape: OutlineShape,
}

/// The final step where the generated distance field is drawn on screen.
struct DrawStep {
    pipeline: wgpu::RenderPipeline,
    uniform_bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
}
#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct DrawUniforms {
    thickness: u32,
    shape: OutlineShape,
    color: [f32; 4],
}

impl OutlineRenderer {
    pub fn new(params: OutlineParams, rend: &super::Renderer) -> Self {
        let gbuf_bind_group_layout = GBuffer::bind_group_layout(rend);

        let gbufs = [
            GBuffer::new(rend, &gbuf_bind_group_layout),
            GBuffer::new(rend, &gbuf_bind_group_layout),
            GBuffer::new(rend, &gbuf_bind_group_layout),
        ];
        let gbuf_size = rend.window_size().into();

        //
        // Init step
        //

        // shaders

        let mesh_init_shader = rend
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("jump flood mesh init"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "shaders/outlines/init.wgsl"
                ))),
            });

        // bind group

        let stencil_bind_group_layout = Self::create_stencil_bind_group_layout(rend);
        let stencil_bind_group = Self::create_stencil_bind_group(rend, &stencil_bind_group_layout);

        // pipeline

        let init_pipeline_layout =
            rend.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("jump flood init"),
                    bind_group_layouts: &[&stencil_bind_group_layout],
                    push_constant_ranges: &[],
                });
        let init_pipeline = rend
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("jump flood init"),
                layout: Some(&init_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_init_shader,
                    entry_point: "vs_main",
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &mesh_init_shader,
                    entry_point: "fs_main",
                    targets: &[Some(GBUF_FORMAT.into())],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                // can't use a stencil test here because we need to sample the stencil buffer
                // in multiple places for antialiasing
                depth_stencil: None,
                // drawing to a non-multisampled internal texture
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });
        let init_step = InitStep {
            pipeline: init_pipeline,
            stencil_bind_group_layout,
            stencil_bind_group,
        };

        //
        // Distance step
        //

        // shaders

        let jfa_shader = rend
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("jump flood"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "shaders/outlines/jump_flood.wgsl"
                ))),
            });

        // bind group & buffers

        let jfa_uniform_buf_size = std::mem::size_of::<DistanceUniforms>() as wgpu::BufferAddress;
        let jfa_uniform_buf = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: jfa_uniform_buf_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("jump flood"),
            mapped_at_creation: false,
        });

        let unif_bind_group_layout =
            rend.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<
                                DistanceUniforms,
                            >()
                                as _),
                        },
                        count: None,
                    }],
                    label: Some("jump flood uniforms"),
                });
        let unif_bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &unif_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: jfa_uniform_buf.as_entire_binding(),
            }],
            label: Some("jump flood uniforms"),
        });

        // pipeline

        let jfa_pipeline_layout =
            rend.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("jump flood"),
                    bind_group_layouts: &[&unif_bind_group_layout, &gbuf_bind_group_layout],
                    push_constant_ranges: &[],
                });
        let jfa_pipeline = rend
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("jump flood"),
                layout: Some(&jfa_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &jfa_shader,
                    entry_point: "vs_main",
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &jfa_shader,
                    entry_point: "fs_main",
                    targets: &[Some(GBUF_FORMAT.into())],
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
        let dist_step = DistanceStep {
            pipeline: jfa_pipeline,
            uniform_bind_group: unif_bind_group,
            uniform_buf: jfa_uniform_buf,
        };

        //
        // Draw step
        //

        // shaders

        let draw_shader = rend
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("outline draw"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "shaders/outlines/draw_outline.wgsl"
                ))),
            });

        // bind group & buffers

        let draw_uniform_buf_size = std::mem::size_of::<DrawUniforms>() as wgpu::BufferAddress;
        let draw_uniform_buf = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: draw_uniform_buf_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("outline draw"),
            mapped_at_creation: false,
        });

        let unif_bind_group_layout =
            rend.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(
                                std::mem::size_of::<DrawUniforms>() as _,
                            ),
                        },
                        count: None,
                    }],
                    label: Some("outline draw uniforms"),
                });
        let unif_bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &unif_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: draw_uniform_buf.as_entire_binding(),
            }],
            label: Some("outline draw uniforms"),
        });

        // pipeline

        let draw_pipeline_layout =
            rend.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("outline draw"),
                    bind_group_layouts: &[
                        &unif_bind_group_layout,
                        // current frame's result
                        &gbuf_bind_group_layout,
                        // last frame's result for temporal smoothing
                        &gbuf_bind_group_layout,
                    ],
                    push_constant_ranges: &[],
                });
        let draw_stencil_test = wgpu::StencilFaceState {
            // only draw on pixels that didn't have the stencil activated
            compare: wgpu::CompareFunction::Equal,
            fail_op: wgpu::StencilOperation::Keep,
            depth_fail_op: wgpu::StencilOperation::Keep,
            pass_op: wgpu::StencilOperation::Keep,
        };
        let draw_pipeline = rend
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("outline draw"),
                layout: Some(&draw_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &draw_shader,
                    entry_point: "vs_main",
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &draw_shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: rend.swapchain_format(),
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: super::depth_buffer::DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: wgpu::StencilState {
                        front: draw_stencil_test,
                        back: draw_stencil_test,
                        read_mask: 0xff,
                        write_mask: 0xff,
                    },
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: rend.multisample_state(),
                multiview: None,
            });
        let draw_step = DrawStep {
            pipeline: draw_pipeline,
            uniform_bind_group: unif_bind_group,
            uniform_buf: draw_uniform_buf,
        };

        Self {
            params,
            gbufs,
            gbuf_bind_group_layout,
            gbuf_size,
            swapchain_gen: rend.generation,

            init_step,
            dist_step,
            draw_step,

            last_gbuf_idx: 0,
        }
    }

    fn create_stencil_bind_group_layout(rend: &super::Renderer) -> wgpu::BindGroupLayout {
        rend.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("outline init stencil buffer"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                    count: None,
                }],
            })
    }

    fn create_stencil_bind_group(
        rend: &super::Renderer,
        bind_group_layout: &wgpu::BindGroupLayout,
    ) -> wgpu::BindGroup {
        rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("outline stencil texture binding"),
            layout: bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &rend.window_depth_buffer.stencil_view,
                ),
            }],
        })
    }

    /// Draw outlines of objects drawn on the screen so far.
    pub fn draw(&mut self, rend: &mut super::Renderer) {
        self.prepare(rend);
        self.run(rend);
    }

    /// Get render textures ready for drawing a new frame of outlines.
    fn prepare(&mut self, rend: &mut super::Renderer) {
        if self.swapchain_gen != rend.generation {
            self.swapchain_gen = rend.generation;
            let window_size: (u32, u32) = rend.window_size().into();
            if window_size != self.gbuf_size {
                self.gbufs = [
                    GBuffer::new(rend, &self.gbuf_bind_group_layout),
                    GBuffer::new(rend, &self.gbuf_bind_group_layout),
                    GBuffer::new(rend, &self.gbuf_bind_group_layout),
                ];
                self.gbuf_size = window_size;
                self.init_step.stencil_bind_group = Self::create_stencil_bind_group(
                    rend,
                    &self.init_step.stencil_bind_group_layout,
                );
            }
        }
    }

    fn run(&mut self, rend: &mut super::Renderer) {
        let first_gbuf_idx = (self.last_gbuf_idx + 1) % GBUF_COUNT;

        //
        // init
        //
        let mut ctx = rend.draw_to_texture(&self.gbufs[first_gbuf_idx].view, None, self.gbuf_size);

        {
            let mut pass = ctx.pass(Some("outline init"));
            pass.set_pipeline(&self.init_step.pipeline);
            pass.set_bind_group(0, &self.init_step.stencil_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        ctx.submit();

        //
        // jump flood
        //

        let pass_count = (self.params.thickness as f64).log2().ceil() as u32;

        let mut source_gbuf_idx = first_gbuf_idx;
        let mut target_gbuf_idx = (source_gbuf_idx + 1) % GBUF_COUNT;
        for pass in (0..pass_count).rev() {
            let step_pixels = 2u32.pow(pass);

            let mut ctx =
                rend.draw_to_texture(&self.gbufs[target_gbuf_idx].view, None, self.gbuf_size);

            let uniforms = DistanceUniforms {
                step_size: step_pixels,
                shape: self.params.shape.normalized(),
            };
            ctx.queue
                .write_buffer(&self.dist_step.uniform_buf, 0, uniforms.as_bytes());

            {
                let mut pass = ctx.pass(Some("jump flood"));
                pass.set_pipeline(&self.dist_step.pipeline);
                pass.set_bind_group(0, &self.dist_step.uniform_bind_group, &[]);
                pass.set_bind_group(1, &self.gbufs[source_gbuf_idx].bind_group, &[]);
                pass.draw(0..3, 0..1);
            }

            ctx.submit();

            std::mem::swap(&mut source_gbuf_idx, &mut target_gbuf_idx);
        }

        //
        // draw to screen
        //

        let mut ctx = rend.draw_to_window();

        let uniforms = DrawUniforms {
            thickness: self.params.thickness,
            shape: self.params.shape.normalized(),
            color: self.params.color,
        };
        ctx.queue
            .write_buffer(&self.draw_step.uniform_buf, 0, uniforms.as_bytes());

        {
            let mut pass = ctx.pass(Some("outline draw"));
            pass.set_pipeline(&self.draw_step.pipeline);
            // stencil to draw only on pixels that didn't have an outlined object on them
            pass.set_stencil_reference(0);
            pass.set_bind_group(0, &self.draw_step.uniform_bind_group, &[]);
            pass.set_bind_group(1, &self.gbufs[source_gbuf_idx].bind_group, &[]);
            pass.set_bind_group(2, &self.gbufs[self.last_gbuf_idx].bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        ctx.submit();

        self.last_gbuf_idx = source_gbuf_idx;
    }
}

//
// Utils
//

const GBUF_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;

/// Fullscreen texture and bind group for sampling it
struct GBuffer {
    view: wgpu::TextureView,
    bind_group: wgpu::BindGroup,
}

impl GBuffer {
    fn new(rend: &super::Renderer, gbuf_bind_group_layout: &wgpu::BindGroupLayout) -> Self {
        let texture = rend.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("outline gbuffer texture"),
            size: wgpu::Extent3d {
                width: rend.window_size().width,
                height: rend.window_size().height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: GBUF_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("outline gbuffer bind group"),
            layout: gbuf_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });

        Self { view, bind_group }
    }

    fn bind_group_layout(rend: &super::Renderer) -> wgpu::BindGroupLayout {
        rend.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                }],
                label: Some("outline gbuffer binding"),
            })
    }
}
