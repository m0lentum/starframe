use crate::{
    graphics::{self as gx, util::GlslMat3},
    {graph, math as m},
};

use std::borrow::Cow;
use zerocopy::{AsBytes, FromBytes};

type Color = [f32; 4];
/// A flat-colored convex polygon shape.
///
/// Concavity will not result in an error but will be rendered incorrectly.
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Circle {
        r: f64,
        points: usize,
        color: Color,
    },
    Rect {
        w: f64,
        h: f64,
        color: Color,
    },
    Capsule {
        hl: f64,
        r: f64,
        points_per_cap: usize,
        color: Color,
    },
}

impl Shape {
    /// Create a Shape that matches the given Collider.
    pub fn from_collider(coll: &crate::physics::Collider, color: Color) -> Self {
        use crate::physics::collision::ColliderShape;
        match coll.shape {
            ColliderShape::Circle { r } => Shape::Circle {
                r,
                points: 16,
                color,
            },
            ColliderShape::Rect { hw, hh } => Shape::Rect {
                w: 2.0 * hw,
                h: 2.0 * hh,
                color,
            },
            ColliderShape::Capsule { hl, r } => Shape::Capsule {
                hl,
                r,
                points_per_cap: 8,
                color,
            },
        }
    }

    pub(self) fn verts(&self, pose: &m::Pose) -> Vec<Vertex> {
        // generate a triangle mesh
        fn as_verts(pts: &[m::Vec2], pose: &m::Pose, color: Color) -> Vec<Vertex> {
            let mut iter = pts.iter().map(|p| *pose * *p).peekable();
            let first = match iter.next() {
                Some(p) => Vertex {
                    position: [p.x as f32, p.y as f32],
                    color,
                },
                None => return Vec::new(),
            };
            let mut verts = Vec::with_capacity((pts.len() - 2) * 3);
            while let Some(curr) = iter.next() {
                if let Some(&next) = iter.peek() {
                    verts.push(first);
                    verts.push(Vertex {
                        position: [curr.x as f32, curr.y as f32],
                        color,
                    });
                    verts.push(Vertex {
                        position: [next.x as f32, next.y as f32],
                        color,
                    });
                }
            }
            verts
        }

        // do it
        match self {
            Shape::Circle { r, points, color } => {
                let angle_incr = 2.0 * std::f64::consts::PI / *points as f64;
                let verts: Vec<m::Vec2> = (0..*points)
                    .map(|i| {
                        let angle = angle_incr * i as f64;
                        m::Vec2::new(r * angle.cos(), r * angle.sin())
                    })
                    .collect();
                as_verts(verts.as_slice(), pose, *color)
            }
            Shape::Rect { w, h, color } => {
                let hw = 0.5 * w;
                let hh = 0.5 * h;
                as_verts(
                    &[
                        m::Vec2::new(hw, hh),
                        m::Vec2::new(-hw, hh),
                        m::Vec2::new(-hw, -hh),
                        m::Vec2::new(hw, -hh),
                    ],
                    pose,
                    *color,
                )
            }
            Shape::Capsule {
                hl,
                r,
                points_per_cap,
                color,
            } => {
                let angle_incr = std::f64::consts::PI / *points_per_cap as f64;
                let verts: Vec<m::Vec2> = (0..=*points_per_cap)
                    .map(|i| {
                        let angle = angle_incr * i as f64;
                        m::Vec2::new(r * angle.sin() + hl, r * angle.cos())
                    })
                    .chain((*points_per_cap..=2 * points_per_cap).map(|i| {
                        let angle = angle_incr * i as f64;
                        m::Vec2::new(r * angle.sin() - hl, r * angle.cos())
                    }))
                    .collect();

                as_verts(verts.as_slice(), pose, *color)
            }
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

pub struct ShapeRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    vert_buf: super::util::DynamicVertexBuffer,
}
impl ShapeRenderer {
    pub fn new(rend: &super::Renderer) -> Self {
        // shaders

        let shader = rend
            .device
            .create_shader_module(&wgpu::ShaderModuleDescriptor {
                label: Some("shape"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders/shape.wgsl"))),
            });

        // bind group & buffers

        let uniform_buf_size = std::mem::size_of::<GlobalUniforms>() as wgpu::BufferAddress;
        let uniform_buf = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: uniform_buf_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("shape uniforms"),
            mapped_at_creation: false,
        });

        let bind_group_layout =
            rend.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[wgpu::BindGroupLayoutEntry {
                        binding: 0, // view matrix
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Uniform,
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<
                                GlobalUniforms,
                            >()
                                as _),
                        },
                        count: None,
                    }],
                    label: Some("shape"),
                });
        let bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
            label: Some("shape"),
        });

        let vertex_buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                // color
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                    shader_location: 1,
                },
            ],
        }];

        // pipeline

        let pipeline_layout = rend
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("shape"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let pipeline = rend
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("shape"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &vertex_buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[rend.swapchain_format().into()],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
            });

        ShapeRenderer {
            pipeline,
            bind_group,
            uniform_buf,
            vert_buf: super::util::DynamicVertexBuffer::new(Some("shape")),
        }
    }

    /// Draw all the alive [`Shape`][self::Shape]s that have associated [`Pose`][crate::math::Pose]s.
    pub fn draw(
        &mut self,
        l_shape: &graph::Layer<Shape>,
        l_pose: &graph::Layer<m::Pose>,
        graph: &graph::Graph,
        camera: &impl gx::camera::Camera,
        ctx: &mut gx::RenderContext,
    ) {
        //
        // Update the uniform buffer
        //

        let uniforms = GlobalUniforms {
            view: camera.view_matrix(ctx.target_size).into(),
        };
        ctx.queue
            .write_buffer(&self.uniform_buf, 0, uniforms.as_bytes());

        //
        // Update the vertex buffer
        //

        let verts: Vec<Vertex> = l_shape
            .iter(graph)
            .filter_map(|s| graph.get_neighbor(&s, l_pose).map(|tr| s.verts(&*tr)))
            .flatten()
            .collect();
        if verts.is_empty() {
            return;
        }

        self.vert_buf.write(ctx, &verts);

        //
        // Render
        //
        {
            let mut pass = ctx.pass();
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vert_buf.slice());
            pass.draw(0..self.vert_buf.len() as u32, 0..1);
        }
    }
}
