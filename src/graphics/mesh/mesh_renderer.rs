use crate::{
    graphics::{
        manager::MeshId,
        util::{GpuMat4, GpuVec3},
        Camera, DepthBuffer, GraphicsManager, RenderContext, Renderer,
    },
    math::{self as m, uv},
};

use itertools::Itertools;
use std::{borrow::Cow, mem::size_of};
use zerocopy::{AsBytes, FromBytes};

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct CameraUniforms {
    view_proj: GpuMat4,
}

#[derive(Clone, Copy, Debug)]
pub struct DirectionalLight {
    pub direct_color: [f32; 3],
    pub ambient_color: [f32; 3],
    pub direction: uv::Vec3,
}

impl Default for DirectionalLight {
    fn default() -> Self {
        Self {
            direct_color: [1.0, 1.0, 1.0],
            ambient_color: [1.0, 1.0, 1.0],
            direction: uv::Vec3::new(0.0, 0.0, 1.0),
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct LightUniforms {
    direct_color: [f32; 3],
    _pad0: u32,
    ambient_color: [f32; 3],
    _pad1: u32,
    direction: [f32; 3],
    _pad2: u32,
}

impl From<DirectionalLight> for LightUniforms {
    fn from(l: DirectionalLight) -> Self {
        Self {
            direct_color: l.direct_color,
            _pad0: 0,
            ambient_color: l.ambient_color,
            _pad1: 0,
            direction: l.direction.normalized().into(),
            _pad2: 0,
        }
    }
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

    camera_buf: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    light_buf: wgpu::Buffer,
    light_bind_group: wgpu::BindGroup,

    // joint storage which grows if needed.
    // not using util::DynamicBuffer because we also need to update a bind group
    // whenever this is reallocated
    joint_storage: wgpu::Buffer,
    joint_capacity: usize,
}

impl MeshRenderer {
    pub fn new(rend: &Renderer) -> Self {
        /// Different pipelines for skinned and unskinned meshes;
        /// this enum helps create them concisely
        enum PipelineVariant {
            Skinned,
            Unskinned,
        }

        // shaders

        let shader = rend
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("mesh"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "../shaders/mesh.wgsl"
                ))),
            });

        //
        // bind groups & buffers
        //

        // camera

        let camera_buf = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<CameraUniforms>() as _,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh camera"),
            mapped_at_creation: false,
        });

        let camera_bind_group_layout =
            rend.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    entries: &[
                        // mesh uniforms
                        wgpu::BindGroupLayoutEntry {
                            binding: 0,
                            visibility: wgpu::ShaderStages::VERTEX,
                            ty: wgpu::BindingType::Buffer {
                                ty: wgpu::BufferBindingType::Uniform,
                                has_dynamic_offset: false,
                                min_binding_size: wgpu::BufferSize::new(
                                    size_of::<CameraUniforms>() as _,
                                ),
                            },
                            count: None,
                        },
                    ],
                    label: Some("skinned mesh camera"),
                });

        let camera_bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skinned mesh camera"),
            layout: &camera_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buf.as_entire_binding(),
            }],
        });

        // light

        let light_buf = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<LightUniforms>() as _,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh lights"),
            mapped_at_creation: false,
        });

        let light_bind_group_layout = rend.device.create_bind_group_layout(
            &wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(size_of::<LightUniforms>() as _),
                    },
                    count: None,
                }],
                label: Some("mesh lights"),
            },
        );

        let light_bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("mesh lights"),
            layout: &light_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: light_buf.as_entire_binding(),
            }],
        });

        // joints

        let joint_storage = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: size_of::<GpuMat4>() as _,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            label: Some("mesh joints"),
            mapped_at_creation: false,
        });

        let joints_bind_group_layout =
            rend.device
                .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
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
                    label: Some("skinned mesh joints"),
                });
        let joints_bind_group = rend.device.create_bind_group(&wgpu::BindGroupDescriptor {
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

            if matches!(variant, PipelineVariant::Skinned) {
                bufs.push(wgpu::VertexBufferLayout {
                    array_stride: size_of::<super::VertexJoints>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        // joints
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Uint16x4,
                            offset: 4 * 3 + 4 * 2,
                            shader_location: 2,
                        },
                        // weights
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 4 * 3 + 4 * 2 + 2 * 4,
                            shader_location: 3,
                        },
                    ],
                });
            }

            bufs.push(wgpu::VertexBufferLayout {
                array_stride: size_of::<Instance>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Uint32,
                        offset: 0,
                        shader_location: 4,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 4,
                        shader_location: 5,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 4 + 4 * 3,
                        shader_location: 6,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 4 + 4 * 3 * 2,
                        shader_location: 7,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 4 + 4 * 3 * 3,
                        shader_location: 8,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x3,
                        offset: 4 + 4 * 3 * 4,
                        shader_location: 9,
                    },
                ],
            });

            bufs
        };

        //
        // pipeline
        //

        let pipeline_layout = rend
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("mesh"),
                bind_group_layouts: &[
                    &camera_bind_group_layout,
                    &light_bind_group_layout,
                    &joints_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });
        let pipeline = |variant: PipelineVariant| {
            rend.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
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
                        targets: &[Some(rend.swapchain_format().into())],
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleList,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: None,
                        ..Default::default()
                    },
                    depth_stencil: Some(DepthBuffer::default_depth_stencil_state()),
                    multisample: rend.multisample_state(),
                    multiview: None,
                })
        };

        Self {
            skinned_pipeline: pipeline(PipelineVariant::Skinned),
            unskinned_pipeline: pipeline(PipelineVariant::Unskinned),
            joints_bind_group,
            joints_bind_group_layout,
            camera_buf,
            camera_bind_group,
            light_buf,
            light_bind_group,
            joint_storage,
            joint_capacity: 0,
        }
    }

    /// Draw all the meshes in the world.
    pub fn draw(
        &mut self,
        manager: &mut GraphicsManager,
        camera: &Camera,
        light: DirectionalLight,
        ctx: &mut RenderContext,
        world: &mut hecs::World,
    ) {
        // resolve string ids for efficient access
        for (_, (id,)) in world.query_mut::<(&mut MeshId,)>() {
            manager.resolve_mesh_id(id);
        }

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

        // TODO: collect skins

        // collect all joint matrices in the world,
        // we'll shove them all in the storage buffer in one go.
        // make sure the iteration order is the same as when rendering
        // so that each mesh gets the correct offset into the array
        let mut joint_matrices: Vec<GpuMat4> = Vec::new();

        // empty bindings not allowed by vulkan,
        // put in one dummy matrix to pass validation
        if joint_matrices.is_empty() {
            joint_matrices.push(uv::Mat4::identity().into());
        }

        // resize joint buffer if needed
        if joint_matrices.len() > self.joint_capacity {
            self.joint_storage = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("skinned mesh joints"),
                size: (size_of::<GpuMat4>() * joint_matrices.len()) as _,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.joints_bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.joints_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.joint_storage.as_entire_binding(),
                }],
                label: Some("skinned mesh joints"),
            });
        }

        //
        // upload uniforms
        //

        let view_proj = camera.view_proj_matrix(ctx.target_size);
        ctx.queue
            .write_buffer(&self.camera_buf, 0, view_proj.as_byte_slice());
        ctx.queue
            .write_buffer(&self.light_buf, 0, LightUniforms::from(light).as_bytes());
        ctx.queue
            .write_buffer(&self.joint_storage, 0, joint_matrices.as_bytes());

        //
        // upload instance data
        //

        let mut curr_idx = 0;
        let mut instances: Vec<Instance> = Vec::new();
        while curr_idx < unskinned_meshes.len() {
            let (id, _) = unskinned_meshes[curr_idx];
            let Some(mesh) = manager.get_mesh_mut(id) else {
                continue;
            };

            while unskinned_meshes[curr_idx].0 == id {
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

            mesh.gpu_data.instance_buf.write(ctx, &instances);
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

            while skinned_meshes[curr_idx].0 == id {
                let (_, pose) = skinned_meshes[curr_idx];

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
                    joint_offset: 0, // TODO
                    model_col0: model.cols[0].xyz().into(),
                    model_col1: model.cols[1].xyz().into(),
                    model_col2: model.cols[2].xyz().into(),
                    model_col3: model.cols[3].xyz().into(),
                });

                curr_idx += 1;
            }

            mesh.gpu_data.instance_buf.write(ctx, &instances);
            mesh.gpu_data.instance_count = instances.len() as u32;
            instances.clear();
        }

        //
        // render
        //

        let mut pass = ctx.encoder.pass(&ctx.target, Some("mesh"));

        pass.set_bind_group(0, &self.camera_bind_group, &[]);
        pass.set_bind_group(1, &self.light_bind_group, &[]);
        pass.set_bind_group(2, &self.joints_bind_group, &[]);

        fn draw_mesh<'pass>(
            pass: &mut wgpu::RenderPass<'pass>,
            manager: &'pass GraphicsManager,
            mesh_id: &MeshId,
        ) {
            let Some(mesh) = manager.get_mesh(mesh_id) else {
                return;
            };

            let material = manager.get_mesh_material(mesh_id);
            pass.set_bind_group(3, &material.bind_group, &[]);

            pass.set_vertex_buffer(0, mesh.gpu_data.vertex_buf.slice(..));
            pass.set_vertex_buffer(1, mesh.gpu_data.instance_buf.slice());
            pass.set_index_buffer(mesh.gpu_data.index_buf.slice(..), wgpu::IndexFormat::Uint16);

            // stencil for outline rendering
            pass.set_stencil_reference(if mesh.has_outline { 1 } else { 0 });

            pass.draw_indexed(
                0..mesh.gpu_data.idx_count,
                0,
                0..mesh.gpu_data.instance_count,
            );
        }

        pass.set_pipeline(&self.unskinned_pipeline);

        for (id, _) in unskinned_meshes {
            draw_mesh(&mut pass, manager, id);
        }

        pass.set_pipeline(&self.skinned_pipeline);

        for (id, _) in skinned_meshes {
            draw_mesh(&mut pass, manager, id);
        }
    }
}
