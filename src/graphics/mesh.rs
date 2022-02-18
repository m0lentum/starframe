use crate::{
    graph::LayerView,
    graphics::{
        self as gx,
        util::{DynamicMeshBuffers, GpuMat3, GpuVec2},
    },
    math as m,
    physics::{collision::ColliderPolygon, Collider},
};

use std::borrow::Cow;
use zerocopy::{AsBytes, FromBytes};

type Color = [f32; 4];

/// Regular shapes that can be used to generate [`Mesh`][self::Mesh]es.
#[derive(Clone, Copy, Debug)]
pub enum MeshShape {
    Circle {
        r: f64,
        points: usize,
    },
    Rect {
        w: f64,
        h: f64,
    },
    Capsule {
        hl: f64,
        r: f64,
        points_per_cap: usize,
    },
}

/// A triangle mesh for rendering.
#[derive(Clone, Debug)]
pub struct Mesh {
    pub color: Color,
    pub(crate) vertices: Vec<m::Vec2>,
}

impl Default for Mesh {
    fn default() -> Self {
        Self {
            color: [1.0; 4],
            vertices: Vec::new(),
        }
    }
}

impl From<MeshShape> for Mesh {
    fn from(shape: MeshShape) -> Self {
        let vertices = match shape {
            MeshShape::Circle { r, points } => {
                let angle_incr = 2.0 * std::f64::consts::PI / points as f64;
                (0..points)
                    .map(|i| {
                        let angle = angle_incr * i as f64;
                        m::Vec2::new(r * angle.cos(), r * angle.sin())
                    })
                    .collect()
            }
            MeshShape::Rect { w, h } => {
                let hw = 0.5 * w;
                let hh = 0.5 * h;
                vec![
                    m::Vec2::new(hw, hh),
                    m::Vec2::new(-hw, hh),
                    m::Vec2::new(-hw, -hh),
                    m::Vec2::new(hw, -hh),
                ]
            }
            MeshShape::Capsule {
                hl,
                r,
                points_per_cap,
            } => {
                let angle_incr = std::f64::consts::PI / points_per_cap as f64;
                (0..=points_per_cap)
                    .map(|i| {
                        let angle = angle_incr * i as f64;
                        m::Vec2::new(r * angle.sin() + hl, r * angle.cos())
                    })
                    .chain((points_per_cap..=2 * points_per_cap).map(|i| {
                        let angle = angle_incr * i as f64;
                        m::Vec2::new(r * angle.sin() - hl, r * angle.cos())
                    }))
                    .collect()
            }
        };

        Self {
            vertices,
            ..Default::default()
        }
    }
}

impl From<Collider> for Mesh {
    fn from(coll: Collider) -> Self {
        let r = coll.shape.circle_r;
        match coll.shape.polygon {
            ColliderPolygon::Point => MeshShape::Circle { r, points: 16 },
            ColliderPolygon::LineSegment { hl } => MeshShape::Capsule {
                hl,
                r,
                points_per_cap: 8,
            },
            // TODO: support circle-rect sums
            ColliderPolygon::Rect { hw, hh } => MeshShape::Rect {
                w: 2.0 * hw,
                h: 2.0 * hh,
            },
        }
        .into()
    }
}

impl Mesh {
    /// Set the color of the mesh in a builder-like fashion.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }
}

//
// Rendering
//

#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct GlobalUniforms {
    view: GpuMat3,
}

#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct Vertex {
    position: GpuVec2,
    color: [f32; 4],
}

pub struct MeshRenderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    bufs: DynamicMeshBuffers<Vertex>,
}
impl MeshRenderer {
    pub fn new(rend: &super::Renderer) -> Self {
        // shaders

        let shader = rend
            .device
            .create_shader_module(&wgpu::ShaderModuleDescriptor {
                label: Some("mesh"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders/mesh.wgsl"))),
            });

        // bind group & buffers

        let uniform_buf_size = std::mem::size_of::<GlobalUniforms>() as wgpu::BufferAddress;
        let uniform_buf = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: uniform_buf_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh uniforms"),
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
                    label: Some("mesh"),
                });
        let bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
            label: Some("mesh"),
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
                label: Some("mesh"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let pipeline = rend
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mesh"),
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
                multiview: None,
            });

        MeshRenderer {
            pipeline,
            bind_group,
            uniform_buf,
            bufs: DynamicMeshBuffers::new(Some("mesh")),
        }
    }

    /// Draw all the [`mesh`][self::Mesh]s that have associated [`Pose`][crate::math::Pose]s.
    pub fn draw(
        &mut self,
        camera: &impl gx::camera::Camera,
        ctx: &mut gx::RenderContext,
        (l_mesh, l_pose): (LayerView<Mesh>, LayerView<m::Pose>),
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

        self.bufs.clear();
        for (mesh, pose) in l_mesh
            .iter()
            .filter_map(|m| m.get_neighbor(&l_pose).map(|p| (m, p)))
        {
            self.bufs.extend(
                mesh.c.vertices.iter().map(|vert| Vertex {
                    position: (*pose.c * *vert).into(),
                    color: mesh.c.color,
                }),
                (1..mesh.c.vertices.len() as u16 - 1).flat_map(|idx| [0, idx, idx + 1]),
            );
        }
        if self.bufs.indices.is_empty() {
            return;
        }

        self.bufs.write(ctx);

        //
        // Render
        //
        {
            let mut pass = ctx.pass(Some("mesh"));
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            self.bufs.set_buffers(&mut pass);
            pass.draw_indexed(self.bufs.index_range(), 0, 0..1);
        }
    }
}
