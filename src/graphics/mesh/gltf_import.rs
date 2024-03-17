//! Utilities for loading meshes, skins and animations from glTF documents.
//!
//! TODOC: how to construct a skinned and animated mesh

use crate::{
    animation::{self as anim, gltf_animation as g_anim},
    graphics::{mesh::skin, texture::TextureData},
    math::uv,
};

use itertools::izip;

// TODO: add proper errors instead of using expect and unimplemented

/// Load a mesh, skin, and animations from a glTF document given as a byte slice.
///
/// If you load your glTF using something else (such as [`assets_manager`](https://docs.rs/assets_manager)),
/// use [`Mesh::from_gltf`][super::Mesh::from_gltf],
/// [`load_skin`][self::load_skin], and [`load_animations`][self::load_animations] separately.
pub fn load_animated_mesh(
    bytes: &[u8],
    rend: &crate::Renderer,
) -> Result<(super::Mesh, skin::Skin, anim::MeshAnimator), gltf::Error> {
    let (doc, bufs, images) = gltf::import_slice(bytes)?;
    let bufs: Vec<&[u8]> = bufs.iter().map(|data| data.0.as_slice()).collect();
    let mesh = super::Mesh::from_gltf(&doc, &bufs, &images, rend);
    let skin = load_skin(&doc, &bufs).expect("no skin in gltf");
    let anim = load_animations(&doc, &bufs).expect("no skin in gltf");

    Ok((mesh, skin, anim))
}

/// Load the vertices of a mesh from a glTF document.
pub fn load_primitives(doc: &gltf::Document, buffers: &[&[u8]]) -> Vec<super::MeshPrimitive> {
    // helper for constructing gltf readers
    let read_buf = |b: gltf::Buffer| Some(&buffers[b.index()][..b.length()]);

    // TODO: support multiple meshes in one document,
    // also probably don't panic if format isn't supported
    let mesh = doc.meshes().next().expect("No meshes in gltf document");
    mesh.primitives()
        .map(|prim| {
            let reader = prim.reader(read_buf);

            let positions = reader
                .read_positions()
                .expect("glTF mesh must have vertices");

            let mut vertices: Vec<super::Vertex> = positions
                .into_iter()
                .map(|p| super::Vertex {
                    position: p.into(),
                    ..Default::default()
                })
                .collect();

            // UVs

            if let Some(tex_coords) = reader.read_tex_coords(0) {
                for (vert, uv) in izip!(&mut vertices, tex_coords.into_f32()) {
                    vert.tex_coords = uv.into();
                }
            }

            // joints

            if let Some(joints) = reader.read_joints(0) {
                for (vert, joints) in izip!(&mut vertices, joints.into_u16()) {
                    vert.joints = joints;
                }
            }

            // weights

            if let Some(weights) = reader.read_weights(0) {
                for (vert, weights) in izip!(&mut vertices, weights.into_f32()) {
                    vert.weights = weights.into();
                }
            }

            let indices: Vec<u16> = reader
                .read_indices()
                .expect("only glTF meshes with indices are supported")
                .into_u32()
                .map(|i| u16::try_from(i).expect("too many indices to fit into u16"))
                .collect();

            super::MeshPrimitive { vertices, indices }
        })
        .collect()
}

pub struct TextureResult<'a> {
    pub diffuse: TextureData<'a>,
    pub normal: TextureData<'a>,
}

pub fn load_textures<'a>(
    doc: &'a gltf::Document,
    images: &'a [gltf::image::Data],
) -> Vec<TextureResult<'a>> {
    let mut textures = Vec::new();

    for material in doc.materials() {
        let mr = material.pbr_metallic_roughness();
        // TODO: support materials without normal maps
        if let (Some(tex_info), Some(normal_info)) =
            (mr.base_color_texture(), material.normal_texture())
        {
            let tex = tex_info.texture();
            let image = &images[tex.source().index()];

            let diffuse_tex = TextureData {
                label: tex.name().map(String::from),
                pixels: &image.pixels,
                format: texture_format_to_wgpu(image.format, true),
                dimensions: (image.width, image.height),
            };

            let norm_tex = normal_info.texture();
            let norm_image = &images[norm_tex.source().index()];
            let normal_tex = TextureData {
                label: norm_tex.name().map(String::from),
                pixels: &norm_image.pixels,
                format: texture_format_to_wgpu(norm_image.format, false),
                dimensions: (norm_image.width, norm_image.height),
            };

            textures.push(TextureResult {
                diffuse: diffuse_tex,
                normal: normal_tex,
            });
        }
    }

    textures
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

/// Load a skin from a glTF document.
///
/// Returns None if there are no skins in the document.
/// Otherwise, returns the first one.
/// TODO: allow multiple skins per document
pub fn load_skin(doc: &gltf::Document, buffers: &[&[u8]]) -> Option<skin::Skin> {
    let read_buf = |b: gltf::Buffer| Some(&buffers[b.index()][..b.length()]);

    let gltf_skin = doc.skins().next()?;
    let mut skin = skin::Skin {
        root_transform: uv::Mat4::identity(),
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

    Some(skin)
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
                    Out::MorphTargetWeights(_) => {
                        unimplemented!("Morph target animations not supported")
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
                            unimplemented!("Morph target animations not supported")
                        }
                    },
                    ty: match target.property() {
                        Prop::Translation | Prop::Scale => g_anim::ChannelType::Vector3,
                        Prop::Rotation => g_anim::ChannelType::Rotor3,
                        Prop::MorphTargetWeights => {
                            unimplemented!("Morph target animations not supported")
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

    Some(anim::MeshAnimator::new(animations))
}
