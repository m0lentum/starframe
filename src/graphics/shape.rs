use crate::{
    core::{
        container::{Container, ContainerInit, IterSeed},
        math as m,
        space::CreationId,
        storage, Transform, TransformFeature,
    },
    graphics::{self as gx, util::GlslMat3},
};

use zerocopy::{AsBytes, FromBytes};

type Color = [f32; 4];
/// A flat-colored convex polygon shape.
///
/// Concavity will not result in an error but will be rendered incorrectly.
pub enum Shape {
    Circle {
        r: f32,
        points: usize,
        color: Color,
    },
    Rect {
        w: f32,
        h: f32,
        color: Color,
    },
    Poly {
        points: Vec<m::Point2>,
        color: Color,
    },
}

impl Shape {
    /// Create a Shape that matches the given Collider.
    pub fn from_collider(coll: &crate::physics::Collider, color: Color) -> Self {
        use crate::physics::ColliderShape;
        match coll.shape() {
            ColliderShape::Circle { r } => Shape::Circle {
                r: *r,
                points: 16,
                color,
            },
            ColliderShape::Rect { hw, hh } => Shape::Rect {
                w: 2.0 * hw,
                h: 2.0 * hh,
                color,
            },
        }
    }

    pub(self) fn verts(&self, tr: &Transform) -> Vec<Vertex> {
        // generate a triangle mesh
        fn as_verts(pts: &[m::Point2], tr: &Transform, color: Color) -> Vec<Vertex> {
            let mut iter = pts.iter().map(|p| tr * *p).peekable();
            let first = match iter.next() {
                Some(p) => Vertex {
                    position: [p.x, p.y],
                    color,
                },
                None => return Vec::new(),
            };
            let mut verts = Vec::with_capacity((pts.len() - 2) * 3);
            while let Some(curr) = iter.next() {
                if let Some(&next) = iter.peek() {
                    verts.push(first);
                    verts.push(Vertex {
                        position: [curr.x, curr.y],
                        color,
                    });
                    verts.push(Vertex {
                        position: [next.x, next.y],
                        color,
                    });
                }
            }
            verts
        };

        // do it
        match self {
            Shape::Circle { r, points, color } => {
                let angle_incr = 2.0 * std::f32::consts::PI / *points as f32;
                let verts: Vec<m::Point2> = (0..*points)
                    .map(|i| {
                        let angle = angle_incr * i as f32;
                        m::Point2::new(r * angle.cos(), r * angle.sin())
                    })
                    .collect();
                as_verts(verts.as_slice(), tr, *color)
            }
            Shape::Rect { w, h, color } => {
                let hw = 0.5 * w;
                let hh = 0.5 * h;
                as_verts(
                    &[
                        m::Point2::new(hw, hh),
                        m::Point2::new(-hw, hh),
                        m::Point2::new(-hw, -hh),
                        m::Point2::new(hw, -hh),
                    ],
                    tr,
                    *color,
                )
            }
            Shape::Poly { points, color } => as_verts(points.as_slice(), tr, *color),
        }
    }
}

//
// Rendering
//

#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct GlobalUniforms {
    view: GlslMat3,
}

#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

pub struct ShapeFeature {
    shapes: Container<storage::DenseVecStorage<Shape>>,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    // we don't create the vertex buffer until in the draw method where we have some objects
    vert_buf: Option<wgpu::Buffer>,
    vert_buf_len: u32,
}
impl ShapeFeature {
    pub fn new(init: ContainerInit, device: &wgpu::Device) -> Self {
        let shapes = Container::new(init);

        // shaders

        let shader_v = include_bytes!("shaders/shape.vert.spv");
        let shader_v_mod = device.create_shader_module(
            &wgpu::read_spirv(std::io::Cursor::new(&shader_v[..])).expect("Failed to read shader"),
        );
        let shader_f = include_bytes!("shaders/shape.frag.spv");
        let shader_f_mod = device.create_shader_module(
            &wgpu::read_spirv(std::io::Cursor::new(&shader_f[..])).expect("Failed to read shader"),
        );

        // bind group & buffers

        let uniform_buf_size = std::mem::size_of::<GlobalUniforms>() as wgpu::BufferAddress;
        let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: uniform_buf_size,
            usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
            label: Some("shape uniforms"),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            bindings: &[wgpu::BindGroupLayoutEntry {
                binding: 0, // view matrix
                visibility: wgpu::ShaderStage::VERTEX,
                ty: wgpu::BindingType::UniformBuffer { dynamic: false },
            }],
            label: Some("shape"),
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            bindings: &[wgpu::Binding {
                binding: 0,
                resource: wgpu::BindingResource::Buffer {
                    buffer: &uniform_buf,
                    range: 0..uniform_buf_size,
                },
            }],
            label: Some("shape"),
        });

        // pipeline

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            bind_group_layouts: &[&bind_group_layout],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            layout: &pipeline_layout,
            vertex_stage: wgpu::ProgrammableStageDescriptor {
                module: &shader_v_mod,
                entry_point: "main",
            },
            fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                module: &shader_f_mod,
                entry_point: "main",
            }),
            rasterization_state: Some(wgpu::RasterizationStateDescriptor {
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: wgpu::CullMode::None,
                depth_bias: 0,
                depth_bias_slope_scale: 0.0,
                depth_bias_clamp: 0.0,
            }),
            primitive_topology: wgpu::PrimitiveTopology::TriangleList,
            color_states: &[wgpu::ColorStateDescriptor {
                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                color_blend: wgpu::BlendDescriptor::REPLACE,
                alpha_blend: wgpu::BlendDescriptor::REPLACE,
                write_mask: wgpu::ColorWrite::ALL,
            }],
            depth_stencil_state: None,
            vertex_state: wgpu::VertexStateDescriptor {
                index_format: wgpu::IndexFormat::Uint16,
                vertex_buffers: &[wgpu::VertexBufferDescriptor {
                    stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::InputStepMode::Vertex,
                    attributes: &[
                        // position
                        wgpu::VertexAttributeDescriptor {
                            format: wgpu::VertexFormat::Float2,
                            offset: 0,
                            shader_location: 0,
                        },
                        // color
                        wgpu::VertexAttributeDescriptor {
                            format: wgpu::VertexFormat::Float4,
                            offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                            shader_location: 1,
                        },
                    ],
                }],
            },
            sample_count: 1,
            sample_mask: !0,
            alpha_to_coverage_enabled: false,
        });

        ShapeFeature {
            shapes,
            pipeline,
            bind_group,
            uniform_buf,
            vert_buf: None,
            vert_buf_len: 0,
        }
    }

    /// Draw all the alive `Shape`s that have associated `Transform`s.
    pub fn draw(
        &mut self,
        iter_seed: IterSeed,
        trs: &TransformFeature,
        camera: &impl gx::camera::Camera,
        ctx: &mut gx::RenderContext,
    ) {
        let iter = || {
            iter_seed
                .overlay(self.shapes.iter())
                .and(trs.iter())
                .into_iter()
        };

        //
        // Update the uniform buffer
        //

        let uniforms = GlobalUniforms {
            view: camera.view_matrix(ctx.target_size).into(),
        };
        let temp_uniform_buf = ctx
            .device
            .create_buffer_with_data(uniforms.as_bytes(), wgpu::BufferUsage::COPY_SRC);
        ctx.encoder.copy_buffer_to_buffer(
            &temp_uniform_buf,
            0,
            &self.uniform_buf,
            0,
            std::mem::size_of::<GlobalUniforms>() as wgpu::BufferAddress,
        );

        //
        // Update the vertex buffer
        //

        let verts: Vec<Vertex> = iter().flat_map(|(s, t)| s.verts(t)).collect();
        let active_verts_len = verts.len() as u32;
        let active_verts_size = active_verts_len as u64 * std::mem::size_of::<Vertex>() as u64;

        let temp_vert_buf = ctx
            .device
            .create_buffer_with_data(verts.as_bytes(), wgpu::BufferUsage::COPY_SRC);

        // Allocate a new buffer if we don't have room for everything
        //
        // TODO: currently this grows on every frame that new shapes have been added,
        // it should reserve some extra space to avoid this
        if self.vert_buf == None || self.vert_buf_len < active_verts_len {
            self.vert_buf = Some(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shape"),
                size: active_verts_size,
                usage: wgpu::BufferUsage::VERTEX | wgpu::BufferUsage::COPY_DST,
            }));
            self.vert_buf_len = active_verts_len;
        }

        // past this point the vertex buffer always exists
        let vert_buf = self.vert_buf.as_ref().unwrap();
        ctx.encoder.copy_buffer_to_buffer(
            &temp_vert_buf,
            0 as wgpu::BufferAddress,
            vert_buf,
            0 as wgpu::BufferAddress,
            active_verts_size,
        );

        //
        // Render
        //
        {
            let mut pass = ctx.pass();
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, vert_buf, 0, 0);
            pass.draw(0..active_verts_len, 0..1);
        }
    }

    /// Add a Shape to an object.
    pub fn add(&mut self, id: CreationId, shape: Shape) {
        self.shapes.insert(id, shape);
    }
}
