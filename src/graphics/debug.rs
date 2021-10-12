//! Utilities for visualizing internal structures like colliders.

// largely copied from ShapeRenderer since this uses the same shader.
// think about abstraction if more stuff needs same or very similar wgpu structures

use std::borrow::Cow;
use zerocopy::{AsBytes, FromBytes};

use crate::{
    graph::LayerView,
    math as m,
    physics::{collision::AABB, Body, Collider},
};

#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct GlobalUniforms {
    view: super::util::GlslMat3,
}

#[repr(C)]
#[derive(Clone, Copy, AsBytes, FromBytes)]
struct Vertex {
    position: [f32; 2],
    color: [f32; 4],
}

/// Renderer to draw
pub struct DebugVisualizer {
    line_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buf: wgpu::Buffer,
    grid_line_buf: super::util::DynamicVertexBuffer,
    grid_mesh_buf: super::util::DynamicVertexBuffer,
    island_line_buf: super::util::DynamicVertexBuffer,
}

impl DebugVisualizer {
    pub fn new(rend: &super::Renderer) -> Self {
        let shader = rend
            .device
            .create_shader_module(&wgpu::ShaderModuleDescriptor {
                label: Some("debug"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("shaders/shape.wgsl"))),
            });

        let uniform_buf_size = std::mem::size_of::<GlobalUniforms>() as wgpu::BufferAddress;
        let uniform_buf = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: uniform_buf_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("debug uniforms"),
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
                    label: Some("debug"),
                });
        let bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            }],
            label: Some("debug"),
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

        let pipeline_layout = rend
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("debug"),
                bind_group_layouts: &[&bind_group_layout],
                push_constant_ranges: &[],
            });
        let pipeline = |topology| {
            rend.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("debug line"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: "vs_main",
                        buffers: &vertex_buffers,
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: "fs_main",
                        targets: &[wgpu::ColorTargetState {
                            format: rend.swapchain_format(),
                            blend: Some(wgpu::BlendState {
                                color: wgpu::BlendComponent {
                                    operation: wgpu::BlendOperation::Add,
                                    src_factor: wgpu::BlendFactor::SrcAlpha,
                                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                                },
                                alpha: wgpu::BlendComponent::REPLACE,
                            }),
                            write_mask: wgpu::ColorWrites::ALL,
                        }],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                })
        };
        let line_pipeline = pipeline(wgpu::PrimitiveTopology::LineList);
        let shape_pipeline = pipeline(wgpu::PrimitiveTopology::TriangleList);

        Self {
            line_pipeline,
            mesh_pipeline: shape_pipeline,
            bind_group,
            uniform_buf,
            grid_line_buf: super::util::DynamicVertexBuffer::new(Some("debug grid lines")),
            grid_mesh_buf: super::util::DynamicVertexBuffer::new(Some("debug grid meshes")),
            island_line_buf: super::util::DynamicVertexBuffer::new(Some("debug island lines")),
        }
    }

    pub fn draw_spatial_index(
        &mut self,
        phys: &crate::Physics,
        camera: &impl super::camera::Camera,
        ctx: &mut super::RenderContext,
    ) {
        // update uniforms

        let uniforms = GlobalUniforms {
            view: camera.view_matrix(ctx.target_size).into(),
        };
        ctx.queue
            .write_buffer(&self.uniform_buf, 0, uniforms.as_bytes());

        // draw populated grid cells

        let hgrid = &phys.spatial_index;
        let verts: Vec<Vertex> = hgrid
            .populated_cells()
            .flat_map(|cell| {
                // more opaque for smaller grid levels
                let alpha = 0.2 * (1.0 - cell.grid_idx as f32 / hgrid.grids.len() as f32);
                let color = [0.8, 0.5 * alpha, alpha, alpha];
                let spacing = hgrid.grids[cell.grid_idx].spacing as f32;
                let min = [
                    hgrid.bounds.min.x as f32 + cell.col_idx as f32 * spacing,
                    hgrid.bounds.min.y as f32 + cell.row_idx as f32 * spacing,
                ];
                let max = [min[0] + spacing, min[1] + spacing];
                std::array::IntoIter::new([
                    [min[0], min[1]],
                    [max[0], min[1]],
                    [min[0], max[1]],
                    [max[0], max[1]],
                    [min[0], max[1]],
                    [max[0], min[1]],
                ])
                .map(move |position| Vertex { position, color })
            })
            .collect();

        self.grid_mesh_buf.write(ctx, &verts);

        {
            let mut pass = ctx.pass();
            pass.set_pipeline(&self.mesh_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.grid_mesh_buf.slice());
            pass.draw(0..self.grid_mesh_buf.len() as u32, 0..1);
        }

        // draw lines

        let bds = hgrid.bounds;
        let verts: Vec<Vertex> = hgrid
            .grids
            .iter()
            .enumerate()
            .flat_map(|(grid_idx, grid)| {
                // less opaque for smaller grid levels
                let alpha = 0.8 * ((grid_idx + 1) as f32 / hgrid.grids.len() as f32);
                let color = [0.0, 0.0, 0.0, alpha];
                let spacing = grid.spacing;
                (0..=grid.column_count)
                    .flat_map(move |col| {
                        let x = (bds.min.x + col as f64 * spacing) as f32;
                        [
                            Vertex {
                                position: [x, bds.min.y as f32],
                                color,
                            },
                            Vertex {
                                position: [x, bds.max.y as f32],
                                color,
                            },
                        ]
                    })
                    .chain((0..=grid.row_count).flat_map(move |row| {
                        let y = (bds.min.y + row as f64 * spacing) as f32;
                        [
                            Vertex {
                                position: [bds.min.x as f32, y],
                                color,
                            },
                            Vertex {
                                position: [bds.max.x as f32, y],
                                color,
                            },
                        ]
                    }))
            })
            .collect();

        self.grid_line_buf.write(ctx, &verts);

        {
            let mut pass = ctx.pass();
            pass.set_pipeline(&self.line_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.grid_line_buf.slice());
            pass.draw(0..self.grid_line_buf.len() as u32, 0..1);
        }
    }

    pub fn draw_islands(
        &mut self,
        phys: &crate::Physics,
        camera: &impl super::camera::Camera,
        ctx: &mut super::RenderContext,
        (l_pose, l_body, l_coll): (LayerView<m::Pose>, LayerView<Body>, LayerView<Collider>),
    ) {
        // update uniforms

        let uniforms = GlobalUniforms {
            view: camera.view_matrix(ctx.target_size).into(),
        };
        ctx.queue
            .write_buffer(&self.uniform_buf, 0, uniforms.as_bytes());

        // draw boxes

        let verts: Vec<Vertex> = phys
            .islands(&l_body)
            .flat_map(|island| {
                let color = [0.3, 0.5, 0.9, 1.0];
                let mut enclosing_aabb = AABB {
                    min: m::Vec2::new(std::f64::MAX, std::f64::MAX),
                    max: m::Vec2::new(std::f64::MIN, std::f64::MIN),
                };
                for body in island {
                    let pose = match body.get_neighbor(&l_pose) {
                        Some(p) => p,
                        // body was deleted
                        None => break,
                    };
                    let pos = pose.c.translation;
                    let r = match body.get_neighbor(&l_coll) {
                        Some(coll) => coll.c.bounding_sphere_r(),
                        None => 0.0,
                    };
                    let r = m::Vec2::new(r, r);
                    enclosing_aabb.min = enclosing_aabb.min.min_by_component(pos - r);
                    enclosing_aabb.max = enclosing_aabb.max.max_by_component(pos + r);
                }
                let min = [enclosing_aabb.min.x as f32, enclosing_aabb.min.y as f32];
                let max = [enclosing_aabb.max.x as f32, enclosing_aabb.max.y as f32];
                std::array::IntoIter::new([
                    [min[0], min[1]],
                    [max[0], min[1]],
                    [max[0], min[1]],
                    [max[0], max[1]],
                    [max[0], max[1]],
                    [min[0], max[1]],
                    [min[0], max[1]],
                    [min[0], min[1]],
                ])
                .map(move |position| Vertex { position, color })
            })
            .collect();

        self.island_line_buf.write(ctx, &verts);

        {
            let mut pass = ctx.pass();
            pass.set_pipeline(&self.line_pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.island_line_buf.slice());
            pass.draw(0..self.island_line_buf.len() as u32, 0..1);
        }
    }
}
