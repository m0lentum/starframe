use crate::{
    core::{
        space::{FeatureSetInit, MasterKey, SpaceReadAccess},
        storage, Container, TransformFeature,
    },
    graphics as gx,
};

use ultraviolet as uv;
use zerocopy::{AsBytes, FromBytes};

type Color = [f32; 4];
/// A flat-colored convex polygon shape.
///
/// Concavity will not result in an error but will be rendered incorrectly.
pub enum Shape {
    Circle { r: f32, color: Color },
    Rect { w: f32, h: f32, color: Color },
    Poly { points: Vec<uv::Vec2>, color: Color },
}

impl Shape {
    /// Create a Shape that matches the given Collider.
    pub fn from_collider(coll: &crate::physics2d::Collider, color: Color) -> Self {
        use crate::physics2d::ColliderShape;
        match coll.shape() {
            ColliderShape::Circle { r } => Shape::Circle { r: *r, color },
            ColliderShape::Rect { hw, hh } => Shape::Rect {
                w: 2.0 * hw,
                h: 2.0 * hh,
                color,
            },
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct Uniforms {
    view: [[f32; 3]; 3],
}

#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
}

pub struct ShapeFeature {
    shapes: Container<storage::DenseVecStorage<Shape>>,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    vert_buf: wgpu::Buffer,
    vert_count: u32,
}
impl ShapeFeature {
    pub fn new(init: FeatureSetInit) -> Self {
        let shapes = Container::new(init);
        let init_uniforms = Uniforms {
            view: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        };

        // shaders

        let device = init.device;
        let o2d_vert = include_bytes!("shaders/shape.vert.spv");
        let o2d_vert_module = device.create_shader_module(
            &wgpu::read_spirv(std::io::Cursor::new(&o2d_vert[..])).expect("Failed to read shader"),
        );
        let o2d_frag = include_bytes!("shaders/shape.frag.spv");
        let o2d_frag_module = device.create_shader_module(
            &wgpu::read_spirv(std::io::Cursor::new(&o2d_frag[..])).expect("Failed to read shader"),
        );

        // bind group & buffers

        let uniform_buf =
            device.create_buffer_with_data(init_uniforms.as_bytes(), wgpu::BufferUsage::UNIFORM);
        let uniform_buf_size = std::mem::size_of::<Uniforms>();

        let initial_vert_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: 10,
            usage: wgpu::BufferUsage::VERTEX,
            label: Some("shape"),
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
                    range: 0..uniform_buf_size as wgpu::BufferAddress,
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
                module: &o2d_vert_module,
                entry_point: "main",
            },
            fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                module: &o2d_frag_module,
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
                vertex_buffers: &[],
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
            vert_buf: initial_vert_buf,
            vert_count: 0,
        }
    }

    pub fn draw(
        &self,
        space: &SpaceReadAccess,
        trs: &TransformFeature,
        ctx: &mut gx::RenderContext,
    ) {
        // TODO: build the vertex array
        // This is currently horribly broken, but committing to save my code

        let mut pass = ctx.pass();
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.set_vertex_buffer(0, &self.vert_buf, 0, 0);
        pass.draw(0..self.vert_count, 0..1);
    }

    /// Add a Shape to an object.
    pub fn add(&mut self, key: MasterKey, shape: Shape) {
        self.shapes.insert(key, shape);
    }
}

const CIRCLE_VERTS_COUNT: u32 = 16;

lazy_static::lazy_static! {
    /// All circles are the same so we can precalculate their vertices
    static ref CIRCLE_VERTS: Vec<[f32; 2]> = {
        let angle_incr = 2.0 * std::f32::consts::PI / CIRCLE_VERTS_COUNT as f32;
        (0..CIRCLE_VERTS_COUNT).map(|i| {
            let angle = angle_incr * i as f32;
            [angle.cos(), angle.sin()]
        }).collect()
    };
}
