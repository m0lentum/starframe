use crate::{
    graphics::{
        light::LightBuffers,
        manager::MeshId,
        material::Material,
        renderer::{DEPTH_FORMAT, SWAPCHAIN_FORMAT},
        util::{GpuMat4, GpuVec4},
        Camera, GraphicsManager,
    },
    math::{self as m, uv},
};

use std::mem::size_of;
use thunderdome as td;
use zerocopy::{AsBytes, FromBytes};

pub struct MeshRenderer {
    skinned_pipeline: wgpu::RenderPipeline,
    unskinned_pipeline: wgpu::RenderPipeline,
    joints_bind_group: wgpu::BindGroup,
    joints_bind_group_layout: wgpu::BindGroupLayout,
    // joint storage which grows if needed.
    // not using util::DynamicBuffer because we also need to update a bind group
    // whenever this is reallocated
    // (this could totally be another thing in util,
    // but hasn't been needed many times yet)
    joint_storage: wgpu::Buffer,
    joint_capacity: usize,
    // same for instance uniforms
    instance_unif_buf: wgpu::Buffer,
    instance_unif_bind_group_layout: wgpu::BindGroupLayout,
    instance_unif_bind_group: wgpu::BindGroup,
    instance_capacity: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct InstanceUniforms {
    model_row0: GpuVec4,
    model_row1: GpuVec4,
    model_row2: GpuVec4,
    joint_offset: u32,
    _pad: [u32; 3],
}

impl MeshRenderer {
    pub(crate) fn new(light_bufs: &LightBuffers) -> Self {
        let device = crate::Renderer::device();

        /// Different pipelines for skinned and unskinned meshes;
        /// this enum helps create them concisely
        enum PipelineVariant {
            Skinned,
            Unskinned,
        }

        // shaders

        let shader = device.create_shader_module(wgpu::include_wgsl!("../shaders/mesh.wgsl"));

        // joints bind group

        let joint_storage = device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<GpuMat4>() as _,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh joints"),
            mapped_at_creation: false,
        });

        let joints_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    // storage buffer for joint matrices
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::VERTEX,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: wgpu::BufferSize::new(size_of::<GpuMat4>() as _),
                        },
                        count: None,
                    },
                ],
                label: Some("mesh joints"),
            });
        let joints_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &joints_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: joint_storage.as_entire_binding(),
            }],
            label: Some("mesh joints"),
        });

        // instance uniforms bind group

        let instance_unif_buf = device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<InstanceUniforms>() as _,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh instance uniforms"),
            mapped_at_creation: false,
        });

        let instance_unif_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("mesh instance uniforms"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(size_of::<InstanceUniforms>() as _),
                    },
                    count: None,
                }],
            });

        let instance_unif_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh instance uniforms"),
            layout: &instance_unif_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: instance_unif_buf.as_entire_binding(),
            }],
        });

        // vertex layouts

        let vertex_buffers = |variant: PipelineVariant| {
            let mut bufs = Vec::new();

            // vertex buffer

            bufs.push(wgpu::VertexBufferLayout {
                array_stride: size_of::<super::Vertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    // position
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 0,
                        shader_location: 0,
                    },
                    // texture coordinates
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 4 * 3,
                        shader_location: 1,
                    },
                ],
            });

            // joints buffer

            if matches!(variant, PipelineVariant::Skinned) {
                bufs.push(wgpu::VertexBufferLayout {
                    array_stride: size_of::<super::VertexJoints>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        // joints
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Uint16x4,
                            offset: 0,
                            shader_location: 2,
                        },
                        // weights
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 2 * 4,
                            shader_location: 3,
                        },
                    ],
                });
            }

            bufs
        };

        //
        // pipeline
        //

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh"),
            bind_group_layouts: &[
                crate::Camera::bind_group_layout(),
                &light_bufs.bind_group_layout,
                &joints_bind_group_layout,
                Material::bind_group_layout(),
                &instance_unif_bind_group_layout,
            ],
            push_constant_ranges: &[],
        });
        let pipeline = |variant: PipelineVariant| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mesh"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: match variant {
                        PipelineVariant::Skinned => "vs_skinned",
                        PipelineVariant::Unskinned => "vs_unskinned",
                    },
                    buffers: &vertex_buffers(variant),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: SWAPCHAIN_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::COLOR,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            })
        };

        Self {
            skinned_pipeline: pipeline(PipelineVariant::Skinned),
            unskinned_pipeline: pipeline(PipelineVariant::Unskinned),
            joints_bind_group,
            joints_bind_group_layout,
            joint_storage,
            joint_capacity: 1,
            instance_unif_buf,
            instance_unif_bind_group_layout,
            instance_unif_bind_group,
            instance_capacity: 1,
        }
    }

    /// Draw all the meshes in the world.
    pub(crate) fn draw<'pass>(
        &'pass mut self,
        pass: &mut wgpu::RenderPass<'pass>,
        manager: &'pass mut GraphicsManager,
        world: &mut hecs::World,
        camera: &'pass Camera,
        light_bufs: &'pass LightBuffers,
    ) {
        let device = crate::Renderer::device();
        let queue = crate::Renderer::queue();

        let meshes_in_world: Vec<(&MeshId, Option<&m::Pose>)> = world
            .query_mut::<(&MeshId, Option<&m::Pose>)>()
            .into_iter()
            .map(|(_, values)| values)
            .collect();

        //
        // gather joint matrices
        //

        // collect all joint matrices in the world,
        // we'll shove them all in the storage buffer in one go.
        // also gather offsets so that meshes can then access the right joints
        let mut joint_matrices: Vec<GpuMat4> = Vec::new();
        let mut skin_offset_map: td::Arena<u32> = td::Arena::new();
        for (id, skin) in manager.skins.iter() {
            skin_offset_map.insert_at(id, joint_matrices.len() as u32);
            joint_matrices.extend(
                skin.evaluate_joint_matrices()
                    .into_iter()
                    .map(GpuMat4::from),
            );
        }

        // empty bindings not allowed by vulkan,
        // put in one dummy matrix to pass validation
        if joint_matrices.is_empty() {
            joint_matrices.push(uv::Mat4::identity().into());
        }

        // resize joint buffer if needed
        if joint_matrices.len() > self.joint_capacity {
            self.joint_storage = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("skinned mesh joints"),
                size: (size_of::<GpuMat4>() * joint_matrices.len()) as _,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.joints_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.joints_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.joint_storage.as_entire_binding(),
                }],
                label: Some("skinned mesh joints"),
            });
        }

        queue.write_buffer(&self.joint_storage, 0, joint_matrices.as_bytes());

        //
        // gather uniforms
        //

        // collect all instance uniforms into a big buffer;
        // we'll use dynamic offsets to bind them
        let mut instance_unifs = Vec::new();
        for (mesh_id, pose) in &meshes_in_world {
            let Some(mesh) = manager.get_mesh_mut(mesh_id) else {
                continue;
            };

            let joint_offset = mesh_id
                .skin
                .and_then(|skin_id| skin_offset_map.get(skin_id).copied())
                .unwrap_or(0);

            let model = match pose {
                Some(&entity_pose) => entity_pose * mesh.offset,
                None => mesh.offset,
            }
            .into_homogeneous_matrix();
            let model_rows = model.transposed();

            instance_unifs.push(InstanceUniforms {
                model_row0: model_rows.cols[0].into(),
                model_row1: model_rows.cols[1].into(),
                model_row2: model_rows.cols[2].into(),
                joint_offset,
                _pad: [0; 3],
            });
        }

        if instance_unifs.len() > self.instance_capacity {
            self.instance_unif_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("mesh instance uniforms"),
                size: (size_of::<InstanceUniforms>() * instance_unifs.len()) as _,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instance_unif_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("mesh instance uniforms"),
                layout: &self.instance_unif_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.instance_unif_buf,
                        offset: 0,
                        size: wgpu::BufferSize::new(size_of::<InstanceUniforms>() as _),
                    }),
                }],
            });
        }

        queue.write_buffer(&self.instance_unif_buf, 0, instance_unifs.as_bytes());

        //
        // render
        //

        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_bind_group(1, &light_bufs.bind_group, &[]);
        pass.set_bind_group(2, &self.joints_bind_group, &[]);

        for (idx, (mesh_id, _)) in meshes_in_world.iter().enumerate() {
            let Some(mesh) = manager.get_mesh(mesh_id) else {
                return;
            };

            let material = manager.get_mesh_material(mesh_id);
            pass.set_bind_group(3, &material.bind_group, &[]);
            pass.set_bind_group(
                4,
                &self.instance_unif_bind_group,
                &[(idx * size_of::<InstanceUniforms>()) as u32],
            );

            pass.set_vertex_buffer(0, mesh.gpu_data.vertex_buf.slice(..));
            if let Some(joints_buf) = &mesh.gpu_data.joints_buf {
                pass.set_vertex_buffer(1, joints_buf.slice(..));
                pass.set_pipeline(&self.skinned_pipeline);
            } else {
                pass.set_pipeline(&self.unskinned_pipeline);
            }
            pass.set_index_buffer(mesh.gpu_data.index_buf.slice(..), wgpu::IndexFormat::Uint16);

            pass.draw_indexed(0..mesh.gpu_data.idx_count, 0, 0..1);
        }
    }
}
