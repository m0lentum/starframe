use crate::{
    graphics::{self as gx, animation as anim, mesh::skinned},
    math::{self as m, uv},
};

use zerocopy::AsBytes;

pub fn import_mesh(rend: &gx::Renderer, doc: &gltf::Document, buffers: &[&[u8]]) -> super::Mesh {
    // helper for constructing gltf readers
    let read_buf = |b: gltf::Buffer| Some(&buffers[b.index()][..b.length()]);

    // TODO: support multiple meshes in one document,
    // also probably don't panic if format isn't supported
    let mesh = doc.meshes().next().expect("No meshes in gltf document");

    //
    // if mesh isn't skinned, return a simple batched mesh
    //

    if doc.skins().next().is_none() {
        let mut vertices: Vec<super::batched::StoredVertex> = Vec::new();
        let mut indices: Vec<u16> = Vec::new();
        for prim in mesh.primitives() {
            let reader = prim.reader(read_buf);

            vertices.extend(
                itertools::izip!(
                    reader
                        .read_positions()
                        .expect("glTF mesh must have vertices"),
                    reader
                        .read_colors(0)
                        .expect("only glTF meshes with vertex colors are supported")
                        .into_rgba_f32(),
                )
                .map(|(pos, col)| super::batched::StoredVertex {
                    position: m::Vec2::new(pos[0] as f64, pos[1] as f64),
                    color: col.into(),
                }),
            );

            indices.extend(
                reader
                    .read_indices()
                    .expect("only glTF meshes with indices are supported")
                    .into_u32()
                    .map(|i| u16::try_from(i).expect("too many indices to fit into u16")),
            );
        }
        return super::batched::BatchedMesh { vertices, indices }.into();
    }

    //
    // mesh primitives
    //

    let mut primitives = Vec::new();

    for prim in mesh.primitives() {
        let reader = prim.reader(read_buf);

        let vertices: Vec<skinned::Vertex> = itertools::izip!(
            reader
                .read_positions()
                .expect("glTF mesh must have vertices"),
            reader
                .read_colors(0)
                .expect("only glTF meshes with vertex colors are supported")
                .into_rgba_f32(),
            reader
                .read_joints(0)
                .expect("if mesh has a skin, it must have joints")
                .into_u16(),
            reader
                .read_weights(0)
                .expect("if mesh has a skin, it must have weights")
                .into_f32(),
        )
        .map(|(pos, col, joints, weights)| skinned::Vertex {
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

        primitives.push(skinned::MeshPrimitive {
            vert_buf,
            idx_buf,
            idx_count,
        });
    }

    //
    // skin
    //

    // this was checked at the start to decide which type of mesh to return
    let gltf_skin = doc.skins().next().unwrap();
    let mut skin = skinned::Skin {
        root_transform: uv::Mat4::identity(),
        joints: Vec::new(),
    };

    // inverse bind matrices
    let reader = gltf_skin.reader(read_buf);
    if let Some(invs) = reader.read_inverse_bind_matrices() {
        skin.joints = itertools::zip(gltf_skin.joints(), invs)
            .map(|(joint, inv_bind)| {
                skinned::Joint {
                    name: joint.name().map(String::from),
                    // parents will be computed once we have all joints
                    parent_idx: None,
                    local_pose: skinned::TransformDecomp::from_parts(
                        joint.transform().decomposed(),
                    ),
                    inv_bind_matrix: inv_bind.into(),
                }
            })
            .collect();
    } else {
        // inverse bind matrices are not provided, meaning they are premultiplied into vertices
        skin.joints = gltf_skin
            .joints()
            .map(|joint| skinned::Joint {
                name: joint.name().map(String::from),
                parent_idx: None,
                local_pose: skinned::TransformDecomp::from_parts(joint.transform().decomposed()),
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
            .filter_map(|gltf_chan| {
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
                        Prop::Translation => skinned::AnimationTarget::Joint {
                            id: target_joint,
                            property: skinned::AnimatedProperty::Translation,
                        },
                        Prop::Rotation => skinned::AnimationTarget::Joint {
                            id: target_joint,
                            property: skinned::AnimatedProperty::Rotation,
                        },
                        Prop::Scale => skinned::AnimationTarget::Joint {
                            id: target_joint,
                            property: skinned::AnimatedProperty::Scale,
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

    skinned::SkinnedMesh {
        primitives,
        skin,
        animations,
        active_anim_idx: None,
        anim_time: 0.0,
        uniforms: None,
    }
    .into()
}
