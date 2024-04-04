use crate::{
    graphics::{
        manager::MeshId,
        renderer::DeferredPass,
        util::{GpuMat4, GpuVec3},
        Camera, GraphicsManager,
    },
    math::{self as m, uv},
};

use itertools::Itertools;
use std::{borrow::Cow, mem::size_of};
use thunderdome as td;
use zerocopy::{AsBytes, FromBytes};

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
pub(crate) struct CameraUniforms {
    view_proj: GpuMat4,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct Instance {
    joint_offset: u32,
    model_col0: GpuVec3,
    model_col1: GpuVec3,
    model_col2: GpuVec3,
    model_col3: GpuVec3,
}

pub struct MeshRenderer {
    skinned_pipeline: wgpu::RenderPipeline,
    unskinned_pipeline: wgpu::RenderPipeline,
    joints_bind_group: wgpu::BindGroup,
    joints_bind_group_layout: wgpu::BindGroupLayout,

    // joint storage which grows if needed.
    // not using util::DynamicBuffer because we also need to update a bind group
    // whenever this is reallocated
    joint_storage: wgpu::Buffer,
    joint_capacity: usize,
}

impl MeshRenderer {
    pub fn new(game: &crate::Game) -> Self {
        let device = crate::Renderer::device();

        /// Different pipelines for skinned and unskinned meshes;
        /// this enum helps create them concisely
        enum PipelineVariant {
            Skinned,
            Unskinned,
        }

        // shaders

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("mesh"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                "../shaders/mesh_geometry.wgsl"
            ))),
        });

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

        // vertex and instance layouts

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

            // instance buffer

            bufs.push(wgpu::VertexBufferLayout {
                array_stride: size_of::<Instance>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    // joint offset
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Uint32,
                        offset: 0,
                        shader_location: 2,
                    },
                    // model matrix column 0
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 4,
                        shader_location: 3,
                    },
                    // model matrix column 1
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 4 + 4 * 3,
                        shader_location: 4,
                    },
                    // model matrix column 2
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 4 + 4 * 3 * 2,
                        shader_location: 5,
                    },
                    // model matrix column 3
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 4 + 4 * 3 * 3,
                        shader_location: 6,
                    },
                ],
            });

            // joints buffer
            // comes after the instance buffer
            // because we want the instance buffer to have the same binding index
            // in both skinned and unskinned pipelines

            if matches!(variant, PipelineVariant::Skinned) {
                bufs.push(wgpu::VertexBufferLayout {
                    array_stride: size_of::<super::VertexJoints>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        // joints
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Uint16x4,
                            offset: 0,
                            shader_location: 7,
                        },
                        // weights
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 2 * 4,
                            shader_location: 8,
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
                &crate::Camera::bind_group_layout(),
                &joints_bind_group_layout,
                &game.graphics.material_res.bind_group_layout,
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
                    targets: &game.renderer.geometry_pass_targets(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(game.renderer.default_depth_stencil_state()),
                multisample: game.renderer.multisample_state(),
                multiview: None,
            })
        };

        Self {
            skinned_pipeline: pipeline(PipelineVariant::Skinned),
            unskinned_pipeline: pipeline(PipelineVariant::Unskinned),
            joints_bind_group,
            joints_bind_group_layout,
            joint_storage,
            joint_capacity: 0,
        }
    }

    /// Draw all the meshes in the world.
    pub fn draw<'pass>(
        &'pass mut self,
        pass: &mut DeferredPass<'pass>,
        manager: &'pass mut GraphicsManager,
        world: &mut hecs::World,
        camera: &'pass Camera,
    ) {
        let device = crate::Renderer::device();
        let queue = crate::Renderer::queue();

        let mut meshes_in_world: Vec<(&MeshId, Option<&m::Pose>)> = world
            .query_mut::<(&MeshId, Option<&m::Pose>)>()
            .into_iter()
            .map(|(_, values)| values)
            .collect_vec();

        // split into skinned and unskinned meshes
        let split_idx = itertools::partition(&mut meshes_in_world, |(id, _)| {
            manager
                .get_mesh(id)
                .and_then(|mesh| mesh.gpu_data.joints_buf.as_ref())
                .is_some()
        });
        let (skinned_meshes, unskinned_meshes) = meshes_in_world.split_at_mut(split_idx);
        // group by mesh id for instanced rendering
        skinned_meshes.sort_by_key(|(id, _)| *id);
        unskinned_meshes.sort_by_key(|(id, _)| *id);

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

        //
        // upload uniforms and instance data
        //

        queue.write_buffer(&self.joint_storage, 0, joint_matrices.as_bytes());

        let mut curr_idx = 0;
        let mut instances: Vec<Instance> = Vec::new();
        while curr_idx < unskinned_meshes.len() {
            let (id, _) = unskinned_meshes[curr_idx];
            let Some(mesh) = manager.get_mesh_mut(id) else {
                continue;
            };

            // meshes were sorted by id earlier; collect all instances of the same mesh
            while curr_idx < unskinned_meshes.len() && unskinned_meshes[curr_idx].0.mesh == id.mesh
            {
                let (_, pose) = unskinned_meshes[curr_idx];

                // build the model matrix and push it into the instance buffer
                let model = {
                    let mesh_pose = match pose {
                        Some(&entity_pose) => entity_pose * mesh.offset,
                        None => mesh.offset,
                    };
                    let pose_3d = m::pose_to_3d(&mesh_pose);
                    pose_3d.into_homogeneous_matrix()
                };
                instances.push(Instance {
                    joint_offset: 0,
                    model_col0: model.cols[0].xyz().into(),
                    model_col1: model.cols[1].xyz().into(),
                    model_col2: model.cols[2].xyz().into(),
                    model_col3: model.cols[3].xyz().into(),
                });

                curr_idx += 1;
            }

            mesh.gpu_data.instance_buf.write(&instances);
            mesh.gpu_data.instance_count = instances.len() as u32;
            instances.clear();
        }

        // same for skinned meshes, with the addition of finding the right joint offset
        curr_idx = 0;
        while curr_idx < skinned_meshes.len() {
            let (id, _) = skinned_meshes[curr_idx];

            let Some(mesh) = manager.get_mesh_mut(id) else {
                continue;
            };

            while curr_idx < skinned_meshes.len() && skinned_meshes[curr_idx].0.mesh == id.mesh {
                let (id, pose) = skinned_meshes[curr_idx];
                let Some(&joint_offset) = id.skin.and_then(|skin_id| skin_offset_map.get(skin_id))
                else {
                    continue;
                };

                // build the model matrix and push it into the instance buffer
                let model = {
                    let mesh_pose = match pose {
                        Some(&entity_pose) => entity_pose * mesh.offset,
                        None => mesh.offset,
                    };
                    let pose_3d = m::pose_to_3d(&mesh_pose);
                    pose_3d.into_homogeneous_matrix()
                };
                instances.push(Instance {
                    joint_offset,
                    model_col0: model.cols[0].xyz().into(),
                    model_col1: model.cols[1].xyz().into(),
                    model_col2: model.cols[2].xyz().into(),
                    model_col3: model.cols[3].xyz().into(),
                });

                curr_idx += 1;
            }

            mesh.gpu_data.instance_buf.write(&instances);
            mesh.gpu_data.instance_count = instances.len() as u32;
            instances.clear();
        }

        //
        // render
        //

        let pass = &mut pass.pass;
        pass.set_bind_group(0, &camera.bind_group, &[]);
        pass.set_bind_group(1, &self.joints_bind_group, &[]);

        fn draw_mesh<'pass>(
            pass: &mut wgpu::RenderPass<'pass>,
            manager: &'pass GraphicsManager,
            mesh_id: &MeshId,
        ) {
            let Some(mesh) = manager.get_mesh(mesh_id) else {
                return;
            };

            let material = manager.get_mesh_material(mesh_id);
            pass.set_bind_group(2, &material.bind_group, &[]);

            pass.set_vertex_buffer(0, mesh.gpu_data.vertex_buf.slice(..));
            pass.set_vertex_buffer(1, mesh.gpu_data.instance_buf.slice());
            if let Some(joints_buf) = &mesh.gpu_data.joints_buf {
                pass.set_vertex_buffer(2, joints_buf.slice(..))
            }
            pass.set_index_buffer(mesh.gpu_data.index_buf.slice(..), wgpu::IndexFormat::Uint16);

            pass.draw_indexed(
                0..mesh.gpu_data.idx_count,
                0,
                0..mesh.gpu_data.instance_count,
            );
        }

        pass.set_pipeline(&self.unskinned_pipeline);

        for (id, _) in unskinned_meshes {
            draw_mesh(pass, manager, id);
        }

        pass.set_pipeline(&self.skinned_pipeline);

        for (id, _) in skinned_meshes {
            draw_mesh(pass, manager, id);
        }
    }
}
