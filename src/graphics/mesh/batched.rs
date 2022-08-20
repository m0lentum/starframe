use crate::{
    graph::LayerView,
    graphics::{
        self as gx,
        util::{DynamicMeshBuffers, GpuMat3, GpuVec2},
        Camera,
    },
    math as m,
    physics::{collision::ColliderPolygon, Collider, ColliderShape},
};

use std::borrow::Cow;
use zerocopy::{AsBytes, FromBytes};

type Color = [f32; 4];
const DEFAULT_COLOR: Color = [1.0; 4];

/// Shape that can be used to generate [`BatchedMesh`][self::BatchedMesh]es.
#[derive(Clone, Copy, Debug)]
pub enum ConvexMeshShape {
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

/// A simple triangle mesh with no skins or animations.
/// All of these in the world are rendered in one draw call.
#[derive(Clone, Debug)]
pub struct BatchedMesh {
    pub(crate) vertices: Vec<StoredVertex>,
    pub(crate) indices: Vec<u16>,
}

/// Vertex stored on the CPU side (as opposed to the Vertex used for rendering).
/// We apply poses and transform into GPU vertices on render.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StoredVertex {
    pub position: m::Vec2,
    pub color: [f32; 4],
}

impl From<ConvexMeshShape> for BatchedMesh {
    fn from(shape: ConvexMeshShape) -> Self {
        let vertices: Vec<StoredVertex> = match shape {
            ConvexMeshShape::Circle { r, points } => {
                let angle_incr = 2.0 * std::f64::consts::PI / points as f64;
                (0..points)
                    .map(|i| {
                        let angle = angle_incr * i as f64;
                        m::Vec2::new(r * angle.cos(), r * angle.sin())
                    })
                    .map(|vert| StoredVertex {
                        position: vert,
                        color: DEFAULT_COLOR,
                    })
                    .collect()
            }
            ConvexMeshShape::Rect { w, h } => {
                let hw = 0.5 * w;
                let hh = 0.5 * h;
                [
                    m::Vec2::new(hw, hh),
                    m::Vec2::new(-hw, hh),
                    m::Vec2::new(-hw, -hh),
                    m::Vec2::new(hw, -hh),
                ]
                .into_iter()
                .map(|vert| StoredVertex {
                    position: vert,
                    color: DEFAULT_COLOR,
                })
                .collect()
            }
            ConvexMeshShape::Capsule {
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
                    .map(|vert| StoredVertex {
                        position: vert,
                        color: DEFAULT_COLOR,
                    })
                    .collect()
            }
        };

        let indices = (1..vertices.len() as u16 - 1)
            .flat_map(|idx| [0, idx, idx + 1])
            .collect();

        Self { vertices, indices }
    }
}

impl From<Collider> for BatchedMesh {
    fn from(coll: Collider) -> Self {
        let mut mesh = Self::from_collider_shape(&coll.shape, 0.1);
        for vert in &mut mesh.vertices {
            vert.position = coll.offset * vert.position;
        }
        mesh
    }
}

impl BatchedMesh {
    pub fn from_collider_shape(shape: &ColliderShape, max_circle_vert_distance: f64) -> Self {
        let mut vertices: Vec<StoredVertex> = Vec::new();

        match shape.polygon {
            ColliderPolygon::Point => {
                use std::f64::consts::TAU;
                let num_increments = (TAU * shape.circle_r / max_circle_vert_distance).ceil();
                let angle_increment = m::Rotor2::from_angle(TAU / num_increments);
                let mut curr_vert = m::Vec2::new(shape.circle_r, 0.0);
                for _ in 0..(num_increments as usize) {
                    vertices.push(StoredVertex {
                        position: curr_vert,
                        color: DEFAULT_COLOR,
                    });
                    curr_vert = angle_increment * curr_vert;
                }
            }
            _ => {
                let edge_count = shape.polygon.edge_count();
                let mut curr_edge_idx = 0;
                let mut prev_edge = shape.polygon.get_edge(0);
                loop {
                    let mut next_edge_idx = curr_edge_idx + 1;
                    let is_last_vert = next_edge_idx >= edge_count;
                    if is_last_vert {
                        next_edge_idx = 0;
                    }

                    let next_edge = shape.polygon.get_edge(next_edge_idx);
                    // we only generate one corner from the mirrored edge,
                    // the rest can be generated by mirroring all vertices created so far
                    // (if the shape is symmetrical, otherwise we've already generated the whole
                    // thing by now)
                    let next_edge = if is_last_vert && shape.polygon.is_rotationally_symmetrical() {
                        next_edge.mirrored()
                    } else {
                        next_edge
                    };

                    if shape.circle_r == 0.0 {
                        // just a polygon, all we need are the ends of the edges
                        vertices.push(StoredVertex {
                            position: next_edge.edge.start,
                            color: DEFAULT_COLOR,
                        });
                    } else {
                        // rounded polygon, generate circle caps offset from the vertex
                        let angle_btw_edges = prev_edge.normal.dot(*next_edge.normal).acos();
                        let num_increments =
                            (angle_btw_edges * shape.circle_r / max_circle_vert_distance).ceil();
                        let angle_increment =
                            m::Rotor2::from_angle(angle_btw_edges / num_increments);

                        let mut curr_offset = shape.circle_r * *prev_edge.normal;
                        vertices.push(StoredVertex {
                            position: next_edge.edge.start + curr_offset,
                            color: DEFAULT_COLOR,
                        });
                        for _ in 0..(num_increments as usize) {
                            curr_offset = angle_increment * curr_offset;
                            vertices.push(StoredVertex {
                                position: next_edge.edge.start + curr_offset,
                                color: DEFAULT_COLOR,
                            });
                        }
                    }
                    prev_edge = next_edge;
                    curr_edge_idx = next_edge_idx;

                    if is_last_vert {
                        break;
                    }
                }

                if shape.polygon.is_rotationally_symmetrical() {
                    let half_vert_count = vertices.len();
                    vertices.extend_from_within(..);
                    for mirror_vert in &mut vertices[half_vert_count..] {
                        mirror_vert.position = -mirror_vert.position;
                    }
                }
            }
        }

        let indices = (1..vertices.len() as u16 - 1)
            .flat_map(|idx| [0, idx, idx + 1])
            .collect();

        Self { vertices, indices }
    }
    /// Set the color of the mesh in a builder-like fashion.
    pub fn with_color(mut self, color: Color) -> Self {
        for vert in &mut self.vertices {
            vert.color = color;
        }
        self
    }
}

//
// Rendering
//

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct GlobalUniforms {
    view: GpuMat3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
pub(super) struct Vertex {
    pub position: GpuVec2,
    pub color: [f32; 4],
}

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    bufs: DynamicMeshBuffers<Vertex>,
}
impl Renderer {
    pub fn new(rend: &gx::Renderer) -> Self {
        // shaders

        let shader = rend
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("mesh"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "../shaders/mesh_batched.wgsl"
                ))),
            });

        // bind group & buffers

        let uniform_buf_size = std::mem::size_of::<GlobalUniforms>() as wgpu::BufferAddress;
        let uniform_buf = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: uniform_buf_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("static mesh uniforms"),
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
                label: Some("static mesh"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let pipeline = rend
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("static mesh"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &vertex_buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(rend.swapchain_format().into())],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(gx::DepthBuffer::default_depth_stencil_state()),
                multisample: rend.multisample_state(),
                multiview: None,
            });

        Renderer {
            pipeline,
            bind_group,
            uniform_buf,
            bufs: DynamicMeshBuffers::new(Some("static mesh")),
        }
    }

    /// Draw all the SimpleBatched meshes in the world in one pass.
    pub fn draw(
        &mut self,
        camera: &Camera,
        ctx: &mut gx::RenderContext,
        (l_mesh, l_pose): (LayerView<super::Mesh>, LayerView<m::Pose>),
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
        // gather the outlined meshes first and non-outlined next and count vertices of each,
        // so we can draw them in two draw calls
        let mut outlined_idx_count: u32 = 0;
        let mut gather = |has_outline: bool| {
            for (mesh, mesh_data) in l_mesh
                .iter()
                .filter(|mesh| mesh.c.has_outline == has_outline)
                .filter_map(|mesh| match &mesh.c.kind {
                    super::MeshKind::SimpleBatched(d) => Some((mesh, d)),
                    _ => None,
                })
            {
                let pose = mesh
                    .get_neighbor(&l_pose)
                    .map(|p| *p.c)
                    .unwrap_or_else(m::Pose::identity);
                self.bufs.extend(
                    mesh_data.vertices.iter().map(|vert| Vertex {
                        position: (pose * vert.position).into(),
                        color: vert.color,
                    }),
                    mesh_data.indices.iter().cloned(),
                );
                if has_outline {
                    outlined_idx_count += mesh_data.indices.len() as u32;
                }
            }
        };
        gather(true);
        gather(false);

        if self.bufs.indices.is_empty() {
            return;
        }

        self.bufs.write(ctx);

        //
        // Render
        //
        {
            let mut pass = ctx.pass(Some("static mesh"));
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            self.bufs.set_buffers(&mut pass);
            // draw outlined
            pass.set_stencil_reference(1);
            pass.draw_indexed(0..outlined_idx_count, 0, 0..1);
            // draw non-outlined
            pass.set_stencil_reference(0);
            pass.draw_indexed(outlined_idx_count..self.bufs.indices.len() as u32, 0, 0..1);
        }
    }
}
