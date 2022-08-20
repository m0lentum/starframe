use crate::{
    graph,
    graphics::{self as gx, animation as anim, util},
    math as m, uv,
};

use std::borrow::Cow;
use zerocopy::{AsBytes, FromBytes};

//
// types
//

#[derive(Debug)]
pub(super) struct MeshPrimitive {
    pub vert_buf: wgpu::Buffer,
    pub idx_buf: wgpu::Buffer,
    pub idx_count: u32,
}

#[derive(Debug)]
pub struct SkinnedMesh {
    pub(super) primitives: Vec<MeshPrimitive>,
    pub(super) skin: Skin,
    pub(super) animations: Vec<anim::Animation<AnimationTarget>>,
    pub(super) active_anim_idx: Option<usize>,
    pub(super) anim_time: f32,
    // storing per-mesh uniform buffers with the meshes,
    // Option because it's not populated until draw
    pub(super) uniforms: Option<MeshUniformBinding>,
}

#[derive(Debug, Clone)]
pub(super) struct Skin {
    pub root_transform: uv::Mat4,
    pub joints: Vec<Joint>,
}

#[derive(Debug, Clone)]
pub(super) struct Joint {
    pub name: Option<String>,
    /// index of the joint's parent joint in the skin's `joints` array
    pub parent_idx: Option<usize>,
    pub local_pose: TransformDecomp,
    pub inv_bind_matrix: uv::Mat4,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TransformDecomp {
    pub pos: uv::Vec3,
    pub rot: uv::Rotor3,
    pub scale: uv::Vec3,
}

impl TransformDecomp {
    pub fn from_parts((pos, rot_quat, scale): ([f32; 3], [f32; 4], [f32; 3])) -> Self {
        Self {
            pos: pos.into(),
            rot: uv::Rotor3::from_quaternion_array(rot_quat),
            scale: scale.into(),
        }
    }

    pub fn as_matrix(&self) -> uv::Mat4 {
        uv::Mat4::from_translation(self.pos)
            * self.rot.into_matrix().into_homogeneous()
            * uv::Mat4::from_nonuniform_scale(self.scale)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum AnimationTarget {
    Joint {
        id: usize,
        property: AnimatedProperty,
    },
    // TODO
    _MorphTarget {
        id: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub(super) enum AnimatedProperty {
    Translation,
    Rotation,
    Scale,
}

#[derive(Debug)]
pub(super) struct MeshUniformBinding {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
pub(super) struct Vertex {
    pub position: util::GpuVec3,
    pub color: util::GpuVec4,
    pub joints: [u16; 4],
    pub weights: util::GpuVec4,
}

//
// rendering
//

#[repr(C)]
#[derive(Debug, Clone, Copy, AsBytes, FromBytes)]
struct MeshUniforms {
    model_view: util::GpuMat3,
    _pad: [u32; 3],
    /// offset into the global joint matrix buffer
    joint_offset: u32,
}

pub struct Renderer {
    pipeline: wgpu::RenderPipeline,
    joints_bind_group: wgpu::BindGroup,
    joints_bind_group_layout: wgpu::BindGroupLayout,
    unif_bind_group_layout: wgpu::BindGroupLayout,
    // joint storage which grows if needed.
    // not using util::DynamicBuffer because we also need to update a bind group
    // whenever this is reallocated
    joint_storage: wgpu::Buffer,
    joint_capacity: usize,
}
impl Renderer {
    pub fn new(rend: &gx::Renderer) -> Self {
        // shaders

        let shader = rend
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("mesh"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!(
                    "../shaders/mesh_skinned.wgsl"
                ))),
            });

        //
        // bind groups & buffers
        //

        // joints

        let joint_storage = rend.device.create_buffer(&wgpu::BufferDescriptor {
            size: std::mem::size_of::<util::GpuMat4>() as _,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            label: Some("skinned mesh joints"),
            mapped_at_creation: false,
        });

        use std::mem::size_of;

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
                                min_binding_size: wgpu::BufferSize::new(
                                    size_of::<util::GpuMat4>() as _
                                ),
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
            label: Some("skinned mesh joints"),
        });

        // layout for per-mesh uniforms, actual bind groups are made later

        let unif_bind_group_layout =
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
                                    size_of::<MeshUniforms>() as _
                                ),
                            },
                            count: None,
                        },
                    ],
                    label: Some("skinned mesh uniforms"),
                });

        // vertices

        let vertex_buffers = [wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                // position is 3D for meshes with depth
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                // color
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: std::mem::size_of::<util::GpuVec3>() as wgpu::BufferAddress,
                    shader_location: 1,
                },
                // joints
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Uint16x4,
                    offset: std::mem::size_of::<(util::GpuVec3, util::GpuVec4)>()
                        as wgpu::BufferAddress,
                    shader_location: 2,
                },
                // weights
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x4,
                    offset: std::mem::size_of::<(util::GpuVec3, [u16; 4], util::GpuVec4)>()
                        as wgpu::BufferAddress,
                    shader_location: 3,
                },
            ],
        }];

        //
        // pipeline
        //

        let pipeline_layout = rend
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("skinned mesh"),
                bind_group_layouts: &[&joints_bind_group_layout, &unif_bind_group_layout],
                push_constant_ranges: &[],
            });
        let pipeline = rend
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("skinned mesh"),
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
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        Self {
            pipeline,
            joints_bind_group,
            joints_bind_group_layout,
            unif_bind_group_layout,
            joint_storage,
            joint_capacity: 0,
        }
    }

    /// Step all animations forward in time.
    pub fn step_time(&mut self, dt: f32, mut l_mesh: graph::LayerViewMut<super::Mesh>) {
        for skin_data in l_mesh.iter_mut().filter_map(|mesh| match &mut mesh.c.kind {
            super::MeshKind::Skinned(skin_data) => Some(skin_data),
            _ => None,
        }) {
            if let Some(anim_idx) = skin_data.active_anim_idx {
                skin_data.anim_time += dt;
                // loop back to the start if we're past the end
                let overshoot = skin_data.anim_time - skin_data.animations[anim_idx].duration;
                if overshoot > 0.0 {
                    skin_data.anim_time = overshoot;
                }
            }
        }
    }

    /// Draw all the skinned meshes in the world.
    pub fn draw(
        &mut self,
        camera: &gx::Camera,
        ctx: &mut gx::RenderContext,
        (mut l_mesh, l_pose): (graph::LayerViewMut<super::Mesh>, graph::LayerView<m::Pose>),
    ) {
        // collect all joint matrices in the world,
        // we'll shove them all in the storage buffer in one go
        let mut joint_matrices: Vec<util::GpuMat4> = Vec::new();
        // cache global poses so we only compute each of them once
        // (rather than every time a child joint is computed)
        let mut global_poses: Vec<Option<uv::Mat4>> = Vec::new();

        for skin_data in l_mesh.iter_mut().filter_map(|mesh| match &mut mesh.c.kind {
            super::MeshKind::Skinned(skin_data) => Some(skin_data),
            _ => None,
        }) {
            // sample animations

            if let Some(anim_idx) = skin_data.active_anim_idx {
                let anim = &skin_data.animations[anim_idx];
                for channel in &anim.channels {
                    match channel.target {
                        AnimationTarget::Joint { id, property } => {
                            let joint = &mut skin_data.skin.joints[id];
                            match property {
                                AnimatedProperty::Translation => {
                                    joint.local_pose.pos = channel.sample_vec3(skin_data.anim_time);
                                }
                                AnimatedProperty::Rotation => {
                                    joint.local_pose.rot =
                                        channel.sample_rotor3(skin_data.anim_time);
                                }
                                AnimatedProperty::Scale => {
                                    joint.local_pose.scale =
                                        channel.sample_vec3(skin_data.anim_time);
                                }
                            }
                        }
                        AnimationTarget::_MorphTarget { .. } => todo!(),
                    }
                }
            }

            // recompute joint matrices

            let skin = &mut skin_data.skin;
            global_poses.clear();
            global_poses.extend(std::iter::repeat(None).take(skin.joints.len()));
            joint_matrices.extend((0..skin.joints.len()).map(|joint_idx| {
                // traverse recursively until an already computed global parent transform is found
                fn populate_parents(
                    joint_idx: usize,
                    joints: &[Joint],
                    global_poses: &mut [Option<uv::Mat4>],
                ) {
                    if let Some(parent_idx) = joints[joint_idx].parent_idx {
                        if global_poses[parent_idx].is_none() {
                            populate_parents(parent_idx, joints, global_poses);
                        }
                        global_poses[joint_idx] = Some(
                            // global pose is guaranteed to exist because we just called
                            // populate_parents if it didn't
                            global_poses[parent_idx].unwrap()
                                * joints[joint_idx].local_pose.as_matrix(),
                        );
                    } else {
                        global_poses[joint_idx] = Some(joints[joint_idx].local_pose.as_matrix());
                    }
                }
                populate_parents(joint_idx, &skin.joints, &mut global_poses);
                let joint_pose = global_poses[joint_idx].unwrap();

                util::GpuMat4::from(
                    skin.root_transform * joint_pose * skin.joints[joint_idx].inv_bind_matrix,
                )
            }));
        }

        // empty bindings not allowed by vulkan,
        // put in one dummy matrix to pass validation
        if joint_matrices.is_empty() {
            joint_matrices.push(uv::Mat4::identity().into());
        }

        // resize joint buffer if needed
        if joint_matrices.len() > self.joint_capacity {
            self.joint_storage = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("skinned mesh joints"),
                size: (std::mem::size_of::<util::GpuMat4>() * joint_matrices.len()) as _,
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

        ctx.queue
            .write_buffer(&self.joint_storage, 0, (&joint_matrices).as_bytes());

        // render the meshes

        let view = camera.view_matrix(ctx.target_size);

        let mut pass = ctx.encoder.pass(&ctx.target, Some("skinned meshes"));
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.joints_bind_group, &[]);

        // joint buffer is shared between all meshes; mesh's offset into it
        let mut joint_offset = 0_u32;
        for mesh in l_mesh
            .iter_mut()
            .filter(|m| matches!(&m.c.kind, super::MeshKind::Skinned(_)))
        {
            let pose = mesh
                .get_neighbor(&l_pose)
                .map(|p| *p.c)
                .unwrap_or_else(m::Pose::identity);
            let mut skin_data = match &mut mesh.c.kind {
                super::MeshKind::Skinned(s) => s,
                // other cases were filtered out on the iterator,
                // but we needed to get the pose before taking this reference to
                // not break borrowing rules
                _ => unreachable!(),
            };
            // mesh uniforms
            // initialize the per-mesh uniform bind group on first render
            if skin_data.uniforms.is_none() {
                let uniform_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                    size: std::mem::size_of::<MeshUniforms>() as _,
                    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                    label: Some("skinned mesh uniforms"),
                    mapped_at_creation: false,
                });
                let unif_bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    layout: &self.unif_bind_group_layout,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buf.as_entire_binding(),
                    }],
                    label: Some("skinned mesh uniforms"),
                });
                skin_data.uniforms = Some(MeshUniformBinding {
                    buffer: uniform_buf,
                    bind_group: unif_bind_group,
                });
            }

            let uniforms = MeshUniforms {
                model_view: (view * (pose * mesh.c.offset).into_homogeneous_matrix()).into(),
                _pad: [0; 3],
                joint_offset,
            };

            // uniforms were just created so they can't be None anymore
            let unif_binding = skin_data.uniforms.as_ref().unwrap();
            ctx.queue
                .write_buffer(&unif_binding.buffer, 0, uniforms.as_bytes());

            // render

            // stencil for outline rendering
            pass.set_stencil_reference(if mesh.c.has_outline { 1 } else { 0 });

            for prim in &skin_data.primitives {
                pass.set_bind_group(1, &unif_binding.bind_group, &[]);
                pass.set_vertex_buffer(0, prim.vert_buf.slice(..));
                pass.set_index_buffer(prim.idx_buf.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..prim.idx_count, 0, 0..1);
            }

            joint_offset += skin_data.skin.joints.len() as u32;
        }
    }
}
