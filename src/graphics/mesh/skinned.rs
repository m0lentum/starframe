use crate::{
    graph,
    graphics::{self as gx, animation as anim, util},
    math as m, uv,
};

use std::borrow::Cow;
use zerocopy::{AsBytes, FromBytes};

/// 3D Mesh with an associated skin deformation, usually animated.
#[derive(Debug)]
pub struct Mesh {
    pub offset: m::Pose,
    primitives: Vec<MeshPrimitive>,
    kind: MeshKind,
}

#[derive(Debug, Clone, Copy)]
pub enum AnimationError {
    NotAnimated,
    FeatureNotFound,
}

impl Mesh {
    /// Activate the animation with the given name, if it exists.
    /// Returns an error if the name doesn't exist or the mesh is not animated at all.
    pub fn activate_animation(&mut self, name: &str) -> Result<(), AnimationError> {
        match &mut self.kind {
            MeshKind::Skinned(skin_data) => {
                match skin_data
                    .animations
                    .iter()
                    .enumerate()
                    .find(|(_, anim)| anim.name.as_deref() == Some(name))
                {
                    Some((idx, _)) => skin_data.active_anim_idx = Some(idx),
                    None => return Err(AnimationError::FeatureNotFound),
                }
            }
            _ => return Err(AnimationError::NotAnimated),
        }
        Ok(())
    }
}

#[derive(Debug)]
struct MeshPrimitive {
    vert_buf: wgpu::Buffer,
    idx_buf: wgpu::Buffer,
    idx_count: u32,
}

#[derive(Debug)]
pub enum MeshKind {
    /// Mesh with a skin and animations attached to it.
    Skinned(SkinData),
    /// Mesh with no skin or animations that is drawn in one draw call
    /// with all other SimpleBatched meshes in the world.
    SimpleBatched,
}

#[derive(Debug)]
pub struct SkinData {
    skin: Skin,
    animations: Vec<anim::Animation<AnimationTarget>>,
    active_anim_idx: Option<usize>,
    anim_time: f32,
    // storing per-mesh uniform buffers with the meshes,
    // Option because it's not populated until draw
    uniforms: Option<MeshUniformBinding>,
}

#[derive(Debug, Clone)]
struct Skin {
    root_transform: uv::Mat4,
    joints: Vec<Joint>,
}

#[derive(Debug, Clone)]
struct Joint {
    name: Option<String>,
    /// index of the joint's parent joint in the skin's `joints` array
    parent_idx: Option<usize>,
    local_pose: TransformDecomp,
    inv_bind_matrix: uv::Mat4,
}

#[derive(Debug, Clone, Copy)]
struct TransformDecomp {
    pos: uv::Vec3,
    rot: uv::Rotor3,
    scale: uv::Vec3,
}

impl TransformDecomp {
    fn from_parts((pos, rot_quat, scale): ([f32; 3], [f32; 4], [f32; 3])) -> Self {
        Self {
            pos: pos.into(),
            rot: uv::Rotor3::from_quaternion_array(rot_quat),
            scale: scale.into(),
        }
    }

    fn as_matrix(&self) -> uv::Mat4 {
        uv::Mat4::from_translation(self.pos)
            * self.rot.into_matrix().into_homogeneous()
            * uv::Mat4::from_nonuniform_scale(self.scale)
    }
}

#[derive(Debug, Clone, Copy)]
enum AnimationTarget {
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
enum AnimatedProperty {
    Translation,
    Rotation,
    Scale,
}

#[derive(Debug)]
struct MeshUniformBinding {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, AsBytes, FromBytes)]
struct Vertex {
    position: util::GpuVec3,
    color: util::GpuVec4,
    joints: [u16; 4],
    weights: util::GpuVec4,
}

impl Mesh {
    #[cfg(feature = "gltf")]
    pub fn from_gltf(rend: &gx::Renderer, doc: &gltf::Document, buffers: &[&[u8]]) -> Self {
        // helper for constructing gltf readers
        let read_buf = |b: gltf::Buffer| Some(&buffers[b.index()][..b.length()]);

        //
        // mesh primitives
        //

        let mut primitives = Vec::new();

        // TODO: support multiple meshes in one document,
        // also probably don't panic if format isn't supported
        let mesh = doc.meshes().next().expect("No meshes in gltf document");
        for prim in mesh.primitives() {
            let reader = prim.reader(read_buf);

            let vertices: Vec<Vertex> = itertools::izip!(
                reader
                    .read_positions()
                    .expect("glTF mesh must have vertices"),
                reader
                    .read_colors(0)
                    .expect("only glTF meshes with vertex colors are supported")
                    .into_rgba_f32(),
                reader
                    .read_joints(0)
                    .expect("only glTF meshes with joints are supported")
                    .into_u16(),
                reader
                    .read_weights(0)
                    .expect("only glTF meshes with weights are supported")
                    .into_f32(),
            )
            .map(|(pos, col, joints, weights)| Vertex {
                position: pos.into(),
                color: col.into(),
                joints,
                weights: weights.into(),
            })
            .collect();

            let indices: Vec<u16> = reader
                .read_indices()
                .expect("only glTF meshes with indices are supported")
                .into_u32()
                .map(|i| u16::try_from(i).expect("too many indices to fit into u16"))
                .collect();
            let idx_count = indices.len() as u32;

            use wgpu::util::DeviceExt;
            let vert_buf = rend
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: mesh.name(),
                    contents: vertices.as_bytes(),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let idx_buf = rend
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: mesh.name(),
                    contents: indices.as_bytes(),
                    usage: wgpu::BufferUsages::INDEX,
                });

            primitives.push(MeshPrimitive {
                vert_buf,
                idx_buf,
                idx_count,
            });
        }

        //
        // skin
        //

        let gltf_skin = match doc.skins().next() {
            // early out if there's no skin
            None => {
                return Self {
                    offset: m::Pose::identity(),
                    primitives,
                    kind: MeshKind::SimpleBatched,
                };
            }
            Some(s) => s,
        };

        let mut skin = Skin {
            root_transform: uv::Mat4::identity(),
            joints: Vec::new(),
        };

        // inverse bind matrices
        let reader = gltf_skin.reader(read_buf);
        if let Some(invs) = reader.read_inverse_bind_matrices() {
            skin.joints = itertools::zip(gltf_skin.joints(), invs)
                .map(|(joint, inv_bind)| {
                    Joint {
                        name: joint.name().map(String::from),
                        // parents will be computed once we have all joints
                        parent_idx: None,
                        local_pose: TransformDecomp::from_parts(joint.transform().decomposed()),
                        inv_bind_matrix: inv_bind.into(),
                    }
                })
                .collect();
        } else {
            // inverse bind matrices are not provided, meaning they are premultiplied into vertices
            skin.joints = gltf_skin
                .joints()
                .map(|joint| Joint {
                    name: joint.name().map(String::from),
                    parent_idx: None,
                    local_pose: TransformDecomp::from_parts(joint.transform().decomposed()),
                    inv_bind_matrix: uv::Mat4::identity(),
                })
                .collect();
        }

        // joint parents

        for (parent_idx, joint) in gltf_skin.joints().enumerate() {
            for child in joint.children() {
                let child_gltf_id = child.index();
                if let Some((child_joint_idx, _)) = gltf_skin
                    .joints()
                    .enumerate()
                    .find(|(_, joint)| joint.index() == child_gltf_id)
                {
                    skin.joints[child_joint_idx].parent_idx = Some(parent_idx);
                }
            }
        }

        // root transform of the skin:
        // we need to traverse the node hierarchy to find any nodes above the root joint.
        // this is because the inverse bind matrices in glTF are relative to the scene root,
        // and we want them relative to the skin root
        if let Some(mut curr_search_node) = gltf_skin.joints().next() {
            loop {
                let parent = doc.nodes().find(|node| {
                    node.children()
                        .any(|child| child.index() == curr_search_node.index())
                });
                if let Some(parent) = parent {
                    skin.root_transform =
                        skin.root_transform * uv::Mat4::from(parent.transform().matrix());
                    curr_search_node = parent;
                } else {
                    break;
                }
            }
        }

        //
        // animations
        //

        let mut animations = Vec::new();

        for gltf_anim in doc.animations() {
            let channels = gltf_anim
                .channels()
                .filter_map(|gltf_chan| -> Option<anim::Channel<AnimationTarget>> {
                    use gltf::animation::util::ReadOutputs as Out;
                    use gltf::animation::Interpolation as Interp;
                    use gltf::animation::Property as Prop;

                    let target = gltf_chan.target();
                    let target_joint = match gltf_skin
                        .joints()
                        .enumerate()
                        .find(|(_, joint)| joint.index() == target.node().index())
                    {
                        Some((joint_idx, _)) => joint_idx,
                        // TODO: morph targets will add another wrinkle to this
                        None => return None,
                    };
                    let sampler = gltf_chan.sampler();
                    let chan_reader = gltf_chan.reader(read_buf);
                    let inputs = chan_reader
                        .read_inputs()
                        .expect("Channel with no inputs")
                        .collect();
                    let mut outputs: Vec<f32> = Vec::new();
                    match chan_reader.read_outputs().expect("Channel with no outputs") {
                        Out::Translations(t) => {
                            outputs.extend(t.flat_map(|t| t.into_iter()));
                        }
                        Out::Rotations(r) => {
                            outputs.extend(r.into_f32().flat_map(|r| r.into_iter()));
                        }
                        Out::Scales(s) => {
                            outputs.extend(s.flat_map(|s| s.into_iter()));
                        }
                        Out::MorphTargetWeights(_) => todo!(),
                    }

                    Some(anim::Channel {
                        target: match target.property() {
                            Prop::Translation => AnimationTarget::Joint {
                                id: target_joint,
                                property: AnimatedProperty::Translation,
                            },
                            Prop::Rotation => AnimationTarget::Joint {
                                id: target_joint,
                                property: AnimatedProperty::Rotation,
                            },
                            Prop::Scale => AnimationTarget::Joint {
                                id: target_joint,
                                property: AnimatedProperty::Scale,
                            },
                            Prop::MorphTargetWeights => todo!(),
                        },
                        ty: match target.property() {
                            Prop::Translation | Prop::Scale => anim::ChannelType::Vector3,
                            Prop::Rotation => anim::ChannelType::Rotor3,
                            Prop::MorphTargetWeights => todo!(),
                        },
                        interpolation: match sampler.interpolation() {
                            Interp::Linear => anim::InterpolationMode::Linear,
                            Interp::Step => anim::InterpolationMode::Step,
                            Interp::CubicSpline => anim::InterpolationMode::CubicSpline,
                        },
                        keyframe_ts: inputs,
                        data: outputs,
                    })
                })
                .collect();

            animations.push(anim::Animation::new(
                gltf_anim.name().map(String::from),
                channels,
            ));
        }

        Self {
            offset: m::Pose::identity(),
            primitives,
            kind: MeshKind::Skinned(SkinData {
                skin,
                animations,
                active_anim_idx: None,
                anim_time: 0.0,
                uniforms: None,
            }),
        }
    }

    #[inline]
    pub fn with_offset(mut self, offset: m::Pose) -> Self {
        self.offset = offset;
        self
    }
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

pub struct SkinnedMeshRenderer {
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
impl SkinnedMeshRenderer {
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
    pub fn step_time(&mut self, dt: f32, mut l_mesh: graph::LayerViewMut<Mesh>) {
        for skin_data in l_mesh.iter_mut().filter_map(|mesh| match &mut mesh.c.kind {
            MeshKind::Skinned(skin_data) => Some(skin_data),
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
        (mut l_mesh, l_pose): (graph::LayerViewMut<Mesh>, graph::LayerView<m::Pose>),
    ) {
        // collect all joint matrices in the world,
        // we'll shove them all in the storage buffer in one go
        let mut joint_matrices: Vec<util::GpuMat4> = Vec::new();
        // cache global poses so we only compute each of them once
        // (rather than every time a child joint is computed)
        let mut global_poses: Vec<Option<uv::Mat4>> = Vec::new();

        for skin_data in l_mesh.iter_mut().filter_map(|mesh| match &mut mesh.c.kind {
            MeshKind::Skinned(skin_data) => Some(skin_data),
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
            .filter(|m| matches!(&m.c.kind, MeshKind::Skinned(_)))
        {
            let pose = mesh
                .get_neighbor(&l_pose)
                .map(|p| *p.c)
                .unwrap_or_else(m::Pose::identity);
            let mut skin_data = match &mut mesh.c.kind {
                MeshKind::Skinned(s) => s,
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
            for prim in &mesh.c.primitives {
                pass.set_bind_group(1, &unif_binding.bind_group, &[]);
                pass.set_vertex_buffer(0, prim.vert_buf.slice(..));
                pass.set_index_buffer(prim.idx_buf.slice(..), wgpu::IndexFormat::Uint16);
                pass.draw_indexed(0..prim.idx_count, 0, 0..1);
            }

            joint_offset += skin_data.skin.joints.len() as u32;
        }
    }
}
