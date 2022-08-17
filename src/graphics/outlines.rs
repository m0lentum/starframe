//! Utilities for drawing outlines around things.

use crate::{
    graph::LayerView,
    graphics::{
        util::{DynamicMeshBuffers, GpuMat3, GpuVec2},
        Camera, StaticMesh,
    },
    math as m,
};

use std::borrow::Cow;
use zerocopy::{AsBytes, FromBytes};

//
// parameters
//

/// Renderer that draws outlines for things using the jump flood algorithm.
pub struct OutlineRenderer {
    pub params: OutlineParams,
    /// two screen-size textures to alternate between for jump flood passes
    gbufs: [GBuffer; 2],
    gbuf_bind_group_layout: wgpu::BindGroupLayout,
    /// store size so we can resize gbuffers if window size changes
    gbuf_size: (u32, u32),

    init_step: InitStep,
    dist_step: DistanceStep,
    draw_step: DrawStep,

    /// state to figure out which gbuffer to draw from and to
    final_gbuf_idx: usize,
}

/// Parameters to configure outline rendering.
#[derive(Clone, Copy, Debug)]
pub struct OutlineParams {
    /// Thickness in pixels.
    pub thickness: u32,
    /// The shape of the outline around corners.
    pub shape: OutlineShape,
}
impl Default for OutlineParams {
    fn default() -> Self {
        Self {
            thickness: 10,
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
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    mesh_bufs: DynamicMeshBuffers<InitVertex>,
}
#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct InitUniforms {
    view: GpuMat3,
}
#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct InitVertex {
    position: GpuVec2,
}

/// The actual jump flooding pass generating a distance field
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

/// The final step where the results are actually drawn on screen.
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
}

impl OutlineRenderer {
    pub fn new(params: OutlineParams, rend: &super::Renderer) -> Self {
        let gbuf_bind_group_layout = GBuffer::bind_group_layout(rend);

        let gbufs = [
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
                    "shaders/outlines/mesh_init.wgsl"
                ))),
            });

        // bind group & buffers

        let uniform_buf_size = std::mem::size_of::<InitUniforms>() as wgpu::BufferAddress;
        let uniform_buf = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: uniform_buf_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("jump flood mesh init"),
            mapped_at_creation: false,
        });

        let bind_group_layout =
            rend.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(
                                std::mem::size_of::<InitUniforms>() as _,
                            ),
                        },
                        count: None,
                    }],
                    label: Some("jump flood mesh init"),
                });
        let init_bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
            label: Some("jump flood mesh init"),
        });

        let init_vertex_buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<InitVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
            ],
        }];

        // pipeline

        let init_pipeline_layout =
            rend.device
                .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                    label: Some("jump flood mesh init"),
                    bind_group_layouts: &[&bind_group_layout],
                    push_constant_ranges: &[],
                });
        let init_pipeline = rend
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("jump flood mesh init"),
                layout: Some(&init_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &mesh_init_shader,
                    entry_point: "vs_main",
                    buffers: &init_vertex_buffers,
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
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });
        let init_step = InitStep {
            pipeline: init_pipeline,
            bind_group: init_bind_group,
            uniform_buf,
            mesh_bufs: DynamicMeshBuffers::new(Some("jump flood mesh init")),
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
                    bind_group_layouts: &[&unif_bind_group_layout, &gbuf_bind_group_layout],
                    push_constant_ranges: &[],
                });
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
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
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

            init_step,
            dist_step,
            draw_step,

            final_gbuf_idx: 0,
        }
    }

    /// Get render textures ready for drawing a new frame of outlines.
    pub fn prepare(&mut self, rend: &mut super::Renderer) {
        let window_size: (u32, u32) = rend.window_size().into();
        if window_size != self.gbuf_size {
            self.gbufs = [
                GBuffer::new(rend, &self.gbuf_bind_group_layout),
                GBuffer::new(rend, &self.gbuf_bind_group_layout),
            ];
            self.gbuf_size = window_size;
        }

        for gbuf in &self.gbufs {
            let mut ctx = rend.draw_to_texture(&gbuf.view, None, self.gbuf_size);
            ctx.clear(NO_DATA_COLOR);
            ctx.submit();
        }
    }

    /// Prepare [`Mesh`][crate::graphics::mesh::Mesh]es for outline drawing.
    pub fn init_meshes(
        &mut self,
        camera: &Camera,
        rend: &mut super::Renderer,
        (l_mesh, l_pose): (LayerView<StaticMesh>, LayerView<m::Pose>),
    ) {
        // update the CPU side vertex buffer

        self.init_step.mesh_bufs.clear();
        for (mesh, pose) in l_mesh
            .iter()
            .filter_map(|m| m.get_neighbor(&l_pose).map(|p| (m, p)))
        {
            self.init_step.mesh_bufs.extend(
                mesh.c.vertices.iter().map(|vert| InitVertex {
                    position: (*pose.c * *vert).into(),
                }),
                (1..mesh.c.vertices.len() as u16 - 1).flat_map(|idx| [0, idx, idx + 1]),
            );
        }
        if self.init_step.mesh_bufs.indices.is_empty() {
            return;
        }

        // only now create a context so we don't do unnecessary work if there's nothing to draw

        let mut ctx = rend.draw_to_texture(&self.gbufs[0].view, None, self.gbuf_size);

        // update the uniform buffer

        let uniforms = InitUniforms {
            view: camera.view_matrix(ctx.target_size).into(),
        };
        ctx.queue
            .write_buffer(&self.init_step.uniform_buf, 0, uniforms.as_bytes());

        // write the updated vertex buffer

        self.init_step.mesh_bufs.write(&ctx);

        // Render

        {
            let mut pass = ctx.pass(Some("outline init"));
            pass.set_pipeline(&self.init_step.pipeline);
            pass.set_bind_group(0, &self.init_step.bind_group, &[]);
            self.init_step.mesh_bufs.set_buffers(&mut pass);
            pass.draw_indexed(self.init_step.mesh_bufs.index_range(), 0, 0..1);
        }

        ctx.submit();
    }

    /// Compute a distance field to draw outlines from.
    pub fn compute(&mut self, rend: &mut super::Renderer) {
        let pass_count = (self.params.thickness as f64).log2().ceil() as u32;

        let mut source_gbuf_idx = 0;
        let mut target_gbuf_idx = 1;
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

        self.final_gbuf_idx = source_gbuf_idx;
    }

    /// Draw the computed outlines to a target.
    pub fn draw(&self, ctx: &mut super::RenderContext) {
        let uniforms = DrawUniforms {
            thickness: self.params.thickness,
            shape: self.params.shape.normalized(),
        };
        ctx.queue
            .write_buffer(&self.draw_step.uniform_buf, 0, uniforms.as_bytes());

        {
            let mut pass = ctx.pass_without_depth(Some("outline draw"));
            pass.set_pipeline(&self.draw_step.pipeline);
            pass.set_bind_group(0, &self.draw_step.uniform_bind_group, &[]);
            pass.set_bind_group(1, &self.gbufs[self.final_gbuf_idx].bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}

//
// Utils
//

const NO_DATA_COLOR: wgpu::Color = wgpu::Color {
    r: -1.0,
    g: -1.0,
    b: -1.0,
    a: -1.0,
};

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
