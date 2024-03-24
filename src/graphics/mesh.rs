pub(crate) mod skin;
pub use skin::Skin;

mod mesh_renderer;
pub use mesh_renderer::{DirectionalLight, MeshRenderer};

//

use crate::{
    graphics as gx,
    math::{self as m},
    physics as phys,
};
use itertools::Itertools;
use zerocopy::{AsBytes, FromBytes};

//
// types
//

/// CPU-side data of a triangle mesh for rendering.
/// Not to be used directly, instead should be converted
/// into a GPU-side [`Mesh`] with [`upload`][Self::upload].
#[derive(Debug, Clone)]
pub struct MeshParams<'a> {
    /// GPU debug label, shown in e.g. Renderdoc.
    pub label: Option<&'a str>,
    /// Offset from the Pose of the entity this mesh is attached to,
    /// or the world origin if it doesn't have a Pose.
    pub offset: m::Pose,
    /// Depth of the mesh in 3D space.
    pub depth: f32,
    /// Whether or not to draw an outline for the mesh when using
    /// [`OutlineRenderer`][crate::OutlineRenderer].
    pub has_outline: bool,
    /// Actual vertex data of the mesh.
    pub data: MeshData,
}

/// CPU-side data of a mesh, possibly with joints and weights for a skin.
#[derive(Debug, Clone, Default)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u16>,
    pub joints: Option<Vec<VertexJoints>>,
}

impl<'a> Default for MeshParams<'a> {
    fn default() -> Self {
        Self {
            label: None,
            offset: m::Pose::default(),
            depth: 0.0,
            has_outline: true,
            data: MeshData::default(),
        }
    }
}

impl<'a> MeshParams<'a> {
    pub fn upload(self, device: &wgpu::Device) -> Mesh {
        use wgpu::util::DeviceExt;
        let vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: self.label,
            contents: self.data.vertices.as_bytes(),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: self.label,
            contents: self.data.indices.as_bytes(),
            usage: wgpu::BufferUsages::INDEX,
        });
        let joints_buf = self.data.joints.as_ref().map(|joints| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: self.label,
                contents: joints.as_bytes(),
                usage: wgpu::BufferUsages::VERTEX,
            })
        });

        let instance_buf =
            gx::util::DynamicBuffer::new(Some("mesh instance"), wgpu::BufferUsages::VERTEX);

        let gpu_data = GpuMeshData {
            vertex_buf,
            index_buf,
            idx_count: self.data.indices.len() as u32,
            joints_buf,
            instance_buf,
            instance_count: 0,
        };

        Mesh {
            offset: self.offset,
            depth: self.depth,
            has_outline: self.has_outline,
            gpu_data,
        }
    }
}

/// Triangle mesh uploaded to the GPU and ready to be rendered.
///
/// Public fields can be mutated and will have an effect on the next render.
/// Vertex data only exists on the GPU at this point and is immutable.
pub struct Mesh {
    pub offset: m::Pose,
    pub depth: f32,
    pub has_outline: bool,
    gpu_data: GpuMeshData,
}

#[derive(Debug)]
pub(crate) struct GpuMeshData {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    idx_count: u32,
    joints_buf: Option<wgpu::Buffer>,
    // instance buffer containing joint offsets and model matrices,
    // allowing the same mesh to be rendered multiple times
    // with potentially different animation states
    instance_buf: gx::util::DynamicBuffer,
    instance_count: u32,
}

/// Position and texture coordinates of a vertex in a mesh.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, AsBytes, FromBytes)]
pub struct Vertex {
    pub position: gx::util::GpuVec3,
    pub tex_coords: gx::util::GpuVec2,
}

/// Joints and weights of a vertex in a skinned mesh.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, AsBytes, FromBytes)]
pub struct VertexJoints {
    pub joints: [u16; 4],
    pub weights: gx::util::GpuVec4,
}

//
// constructors
//

/// Shape that can be used to generate [`Mesh`][self::Mesh]es.
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

impl MeshData {
    pub fn from_collider_shape(shape: &phys::ColliderShape, max_circle_vert_distance: f64) -> Self {
        let mut vertices: Vec<m::Vec2> = Vec::new();

        match shape.polygon {
            phys::ColliderPolygon::Point => {
                use std::f64::consts::TAU;
                let num_increments = (TAU * shape.circle_r / max_circle_vert_distance).ceil();
                let angle_increment = m::Rotor2::from_angle(TAU / num_increments);
                let mut curr_vert = m::Vec2::new(shape.circle_r, 0.0);
                for _ in 0..(num_increments as usize) {
                    vertices.push(curr_vert);
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
                        vertices.push(next_edge.edge.start);
                    } else {
                        // rounded polygon, generate circle caps offset from the vertex
                        let angle_btw_edges = prev_edge.normal.dot(*next_edge.normal).acos();
                        let num_increments =
                            (angle_btw_edges * shape.circle_r / max_circle_vert_distance).ceil();
                        let angle_increment =
                            m::Rotor2::from_angle(angle_btw_edges / num_increments);

                        let mut curr_offset = shape.circle_r * *prev_edge.normal;
                        vertices.push(next_edge.edge.start + curr_offset);
                        for _ in 0..(num_increments as usize) {
                            curr_offset = angle_increment * curr_offset;
                            vertices.push(next_edge.edge.start + curr_offset);
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
                        *mirror_vert = -*mirror_vert;
                    }
                }
            }
        }

        use itertools::MinMaxResult as MM;
        let width = match vertices.iter().map(|v| v.x).minmax() {
            MM::MinMax(l, u) => u - l,
            // malformed mesh, this shouldn't happen.
            // just set it to something other than zero
            _ => 1.,
        };
        let height = match vertices.iter().map(|v| v.y).minmax() {
            MM::MinMax(l, u) => u - l,
            _ => 1.,
        };

        let vertices: Vec<Vertex> = vertices
            .iter()
            .map(|&vert| Vertex {
                position: vert.into(),
                tex_coords: m::Vec2::new(
                    (vert.x + width / 2.) / width,
                    -(vert.y + height / 2.) / height,
                )
                .into(),
            })
            .collect();

        let indices = (1..vertices.len() as u16 - 1)
            .flat_map(|idx| [0, idx, idx + 1])
            .collect();

        MeshData {
            vertices,
            indices,
            joints: None,
        }
    }
}

impl From<ConvexMeshShape> for MeshData {
    fn from(shape: ConvexMeshShape) -> Self {
        // helper for generating uv coordinates which start at the top left
        let flip_y = |v: m::Vec2| m::Vec2::new(v.x, -v.y);

        let vertices: Vec<Vertex> = match shape {
            ConvexMeshShape::Circle { r, points } => {
                let diameter = 2. * r;
                let angle_incr = 2.0 * std::f64::consts::PI / points as f64;
                (0..points)
                    .map(|i| {
                        let angle = angle_incr * i as f64;
                        m::Vec2::new(r * angle.cos(), r * angle.sin())
                    })
                    .map(|vert| Vertex {
                        position: vert.into(),
                        tex_coords: flip_y((vert + m::Vec2::new(r, r)) / diameter).into(),
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
                .map(|vert| Vertex {
                    position: vert.into(),
                    tex_coords: flip_y(m::Vec2::new((vert.x + hw) / w, (vert.y + hh) / h)).into(),
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
                    .map(|vert| Vertex {
                        position: vert.into(),
                        tex_coords: flip_y(m::Vec2::new(
                            (vert.x + hl + r) / (2. * (hl + r)),
                            (vert.y + r) / (2. * r),
                        ))
                        .into(),
                    })
                    .collect()
            }
        };

        let indices = (1..vertices.len() as u16 - 1)
            .flat_map(|idx| [0, idx, idx + 1])
            .collect();

        MeshData {
            vertices,
            indices,
            joints: None,
        }
    }
}

impl From<phys::Collider> for MeshData {
    fn from(coll: phys::Collider) -> Self {
        Self::from_collider_shape(&coll.shape, 0.1)
    }
}
