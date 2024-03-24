//! Utilities for loading meshes, skins and animations from glTF documents.

use super::MaterialId;
use crate::{
    animation::{self as anim, gltf_animation as g_anim},
    graphics::{
        material::{MaterialParams, TextureData},
        mesh::skin,
        mesh::{self, MeshPrimitive},
    },
    math::uv,
};

use itertools::izip;

pub fn load_mesh_primitive<'doc>(
    buffers: &'doc [&[u8]],
    prim: gltf::Primitive<'doc>,
) -> MeshPrimitive {
    // helper for constructing gltf readers
    let read_buf = |b: gltf::Buffer| Some(&buffers[b.index()][..b.length()]);

    let reader = prim.reader(read_buf);

    let positions = reader
        .read_positions()
        .expect("Mesh primitive without positions");

    let mut vertices: Vec<mesh::Vertex> = positions
        .into_iter()
        .map(|p| mesh::Vertex {
            position: p.into(),
            tex_coords: [0., 0.].into(),
        })
        .collect();

    if let Some(tex_coords) = reader.read_tex_coords(0) {
        for (vert, uv) in izip!(&mut vertices, tex_coords.into_f32()) {
            vert.tex_coords = uv.into();
        }
    }

    let indices: Vec<u16> = if let Some(idx_read) = reader.read_indices() {
        idx_read
            .into_u32()
            .filter_map(|i| u16::try_from(i).ok())
            .collect()
    } else {
        (0..vertices.len())
            .filter_map(|i| u16::try_from(i).ok())
            .collect()
    };

    let joints = reader.read_joints(0).and_then(|joints| {
        reader.read_weights(0).map(|weights| {
            izip!(joints.into_u16(), weights.into_f32())
                .map(|(j, w)| mesh::VertexJoints {
                    joints: j,
                    weights: w.into(),
                })
                .collect()
        })
    });

    MeshPrimitive {
        vertices,
        indices,
        joints,
        // this will be filled in by the manager method calling this
        material: MaterialId::default(),
    }
}

pub fn load_material<'doc>(
    images: &'doc [gltf::image::Data],
    material: gltf::Material<'doc>,
) -> MaterialParams<'doc> {
    let mr = material.pbr_metallic_roughness();

    let base_color = Some(mr.base_color_factor());
    let diffuse_tex = mr.base_color_texture().map(|tex_info| {
        let tex = tex_info.texture();
        let image = &images[tex.source().index()];
        TextureData {
            label: tex.name().map(String::from),
            pixels: &image.pixels,
            format: texture_format_to_wgpu(image.format, true),
            dimensions: (image.width, image.height),
        }
    });

    let normal_tex = material.normal_texture().map(|normal_info| {
        let tex = normal_info.texture();
        let image = &images[tex.source().index()];
        TextureData {
            label: tex.name().map(String::from),
            pixels: &image.pixels,
            format: texture_format_to_wgpu(image.format, false),
            dimensions: (image.width, image.height),
        }
    });

    MaterialParams {
        base_color,
        diffuse_tex,
        normal_tex,
    }
}

/// Convert a gltf texture format to the wgpu equivalent.
///
/// Gltf doesn't tell you if the textures are SRGB or linear as far as I can tell,
/// so we assume SRGB for diffuse textures and linear for normal maps.
fn texture_format_to_wgpu(format: gltf::image::Format, is_srgb: bool) -> wgpu::TextureFormat {
    use gltf::image::Format as GF;
    use wgpu::TextureFormat as WF;
    match format {
        GF::R8 => WF::R8Unorm,
        GF::R8G8 => WF::Rg8Unorm,
        GF::R8G8B8 => unimplemented!("RGB textures without alpha are not supported"),
        GF::R8G8B8A8 => {
            if is_srgb {
                WF::Rgba8UnormSrgb
            } else {
                WF::Rgba8Unorm
            }
        }
        GF::R16 => WF::R16Unorm,
        GF::R16G16 => WF::Rg16Unorm,
        GF::R16G16B16 => unimplemented!("RGB textures without alpha are not supported"),
        GF::R16G16B16A16 => WF::Rgba16Unorm,
        GF::R32G32B32FLOAT => unimplemented!("RGB textures without alpha are not supported"),
        GF::R32G32B32A32FLOAT => WF::Rgba32Float,
    }
}

/// Load skins from a glTF document.
pub fn load_skin<'doc>(
    buffers: &'doc [&[u8]],
    gltf_skin: gltf::Skin<'doc>,
    // root transform needed
    // because the inverse bind matrices in glTF are relative to the scene root,
    // and we want them relative to the skin root
    root_transform: uv::Mat4,
) -> skin::Skin {
    let read_buf = |b: gltf::Buffer| Some(&buffers[b.index()][..b.length()]);

    let mut skin = skin::Skin {
        root_transform,
        joints: Vec::new(),
    };

    // inverse bind matrices
    let reader = gltf_skin.reader(read_buf);
    if let Some(invs) = reader.read_inverse_bind_matrices() {
        skin.joints = itertools::zip(gltf_skin.joints(), invs)
            .map(|(joint, inv_bind)| {
                skin::Joint {
                    name: joint.name().map(String::from),
                    // parents will be computed once we have all joints
                    parent_idx: None,
                    inv_bind_matrix: inv_bind.into(),
                    local_pose: skin::TransformDecomp::from_parts(joint.transform().decomposed()),
                    joint_matrix: Default::default(),
                }
            })
            .collect();
    } else {
        // inverse bind matrices are not provided, meaning they are premultiplied into vertices
        skin.joints = gltf_skin
            .joints()
            .map(|joint| skin::Joint {
                name: joint.name().map(String::from),
                parent_idx: None,
                inv_bind_matrix: uv::Mat4::identity(),
                local_pose: skin::TransformDecomp::from_parts(joint.transform().decomposed()),
                joint_matrix: Default::default(),
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

    skin
}

/// Load all animations associated with a skin in a glTF document.
///
/// This assumes the skin is the first one in the document
/// (usually there is only one skin).
/// TODO: handle cases with multiple skins per doc
pub fn load_animations(doc: &gltf::Document, buffers: &[&[u8]]) -> Option<anim::MeshAnimator> {
    let read_buf = |b: gltf::Buffer| Some(&buffers[b.index()][..b.length()]);

    let gltf_skin = doc.skins().next()?;

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
                let inputs = chan_reader.read_inputs()?.collect();
                let mut outputs: Vec<f32> = Vec::new();
                match chan_reader.read_outputs()? {
                    Out::Translations(t) => {
                        outputs.extend(t.flat_map(|t| t.into_iter()));
                    }
                    Out::Rotations(r) => {
                        outputs.extend(r.into_f32().flat_map(|r| r.into_iter()));
                    }
                    Out::Scales(s) => {
                        outputs.extend(s.flat_map(|s| s.into_iter()));
                    }
                    Out::MorphTargetWeights(_) => {
                        eprintln!("Morph target animations not supported");
                        return None;
                    }
                }

                Some(g_anim::Channel {
                    target: match target.property() {
                        Prop::Translation => g_anim::Target::Joint {
                            id: target_joint,
                            property: g_anim::AnimatedProperty::Translation,
                        },
                        Prop::Rotation => g_anim::Target::Joint {
                            id: target_joint,
                            property: g_anim::AnimatedProperty::Rotation,
                        },
                        Prop::Scale => g_anim::Target::Joint {
                            id: target_joint,
                            property: g_anim::AnimatedProperty::Scale,
                        },
                        Prop::MorphTargetWeights => {
                            eprintln!("Morph target animations not supported");
                            return None;
                        }
                    },
                    ty: match target.property() {
                        Prop::Translation | Prop::Scale => g_anim::ChannelType::Vector3,
                        Prop::Rotation => g_anim::ChannelType::Rotor3,
                        Prop::MorphTargetWeights => {
                            eprintln!("Morph target animations not supported");
                            return None;
                        }
                    },
                    interpolation: match sampler.interpolation() {
                        Interp::Linear => g_anim::InterpolationMode::Linear,
                        Interp::Step => g_anim::InterpolationMode::Step,
                        Interp::CubicSpline => g_anim::InterpolationMode::CubicSpline,
                    },
                    keyframe_ts: inputs,
                    data: outputs,
                })
            })
            .collect();

        animations.push(g_anim::GltfAnimation::new(
            gltf_anim.name().map(String::from),
            channels,
        ));
    }

    // TODO: how do we associate animations with their meshes when there are many in a doc?
    Some(anim::MeshAnimator::new(animations))
}
