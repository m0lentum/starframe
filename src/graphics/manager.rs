use std::collections::HashMap;
use thunderdome as td;

use super::{
    animation::{animator::Animator, gltf_animation::GltfAnimation},
    material::{Material, MaterialParams, MaterialResources},
    mesh::{Mesh, MeshParams},
    Renderer, Skin,
};
use crate::math::uv;

#[cfg(feature = "gltf")]
mod gltf_import;

//
// id types
//

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MeshId {
    pub(crate) mesh: td::Index,
    // mesh id also refers to a skin if it has one,
    // because there can be multiple instances of a skin with different animations
    // for one instance of a mesh
    pub(crate) skin: Option<td::Index>,
}

/// Skin ids for internal use only,
/// users don't see them directly
/// and instead operate on meshes and animations
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SkinId(pub(crate) td::Index);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnimationId(td::Index);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnimatorId(td::Index);

//
// manager itself
//

pub struct GraphicsManager {
    meshes: td::Arena<Mesh>,
    /// map from mesh names to mesh ids
    mesh_name_map: HashMap<String, td::Index>,
    /// map from mesh ids to skin ids
    mesh_skin_map: td::Arena<td::Index>,
    /// map from mesh ids to material ids
    mesh_material_map: td::Arena<td::Index>,

    /// skins need to be iterated over and addressed by index in the mesh renderer,
    /// hence pub(crate)
    pub(crate) skins: td::Arena<Skin>,
    animations: td::Arena<GltfAnimation>,
    /// map from animation names to animation ids
    anim_name_map: HashMap<String, td::Index>,
    /// map from animations to target skins
    anim_target_map: td::Arena<td::Index>,
    animators: td::Arena<Animator>,

    materials: td::Arena<Material>,
    pub(crate) material_res: MaterialResources,
    default_material: Material,
}

/// Error when loading assets from a glTF document.
#[cfg(feature = "gltf")]
#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("Invalid file name")]
    InvalidFileName,
    #[error("Failed to open the file at the given path")]
    IoError(#[from] std::io::Error),
    #[error("Failed to read glTF document")]
    GltfError(#[from] gltf::Error),
}

impl GraphicsManager {
    /// Create a new graphics manager.
    #[inline]
    pub fn new(rend: &Renderer) -> Self {
        let material_res = MaterialResources::new(rend);
        let default_material = Material::new(
            rend,
            &material_res,
            MaterialParams {
                base_color: Some([1.; 4]),
                diffuse_tex: None,
                normal_tex: None,
            },
        );

        Self {
            meshes: td::Arena::new(),
            mesh_name_map: HashMap::new(),
            mesh_skin_map: td::Arena::new(),
            mesh_material_map: td::Arena::new(),

            skins: td::Arena::new(),
            animations: td::Arena::new(),
            anim_name_map: HashMap::new(),
            anim_target_map: td::Arena::new(),
            animators: td::Arena::new(),

            materials: td::Arena::new(),
            material_res,
            default_material,
        }
    }

    /// Load all graphics assets (meshes, skins, materials, animations)
    /// in a glTF document.
    #[cfg(feature = "gltf")]
    pub fn load_gltf(
        &mut self,
        rend: &Renderer,
        path: impl AsRef<std::path::Path>,
    ) -> Result<(), LoadError> {
        let path = path.as_ref();
        let file_bytes = std::fs::read(path)?;

        let file_stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or(LoadError::InvalidFileName)?;
        // asset ids are names of the gltf node prefixed with file name
        // (the assumption being that names are unique, as is enforced by Blender)
        let name_to_id = |name: &str| format!("{file_stem}.{name}");

        let (doc, bufs, images) = gltf::import_slice(file_bytes)?;
        let bufs: Vec<&[u8]> = bufs.iter().map(|data| data.0.as_slice()).collect();

        // node transforms in world space, evaluated from the scene graph
        let mut node_transforms_world = vec![uv::Mat4::identity(); doc.nodes().count()];
        // meshes are associated with skins in the node hierarchy,
        // they don't have a direct link in the mesh/skin structures themselves.
        // we only support meshes with a single skin stored in the same glTF file,
        // so we can collect the associations into a map here.
        // this maps mesh indices to skin indices,
        // which we'll convert to asset ids later
        let mut mesh_skin_map: HashMap<usize, usize> = HashMap::new();
        // recursive traversal of the scene graph to get node data
        for scene in doc.scenes() {
            for node in scene.nodes() {
                struct TraversalContext<'a> {
                    node_transforms_world: &'a mut [uv::Mat4],
                    mesh_skin_map: &'a mut HashMap<usize, usize>,
                }
                fn traverse_node(
                    ctx: &mut TraversalContext<'_>,
                    node: gltf::Node<'_>,
                    parent_transform: uv::Mat4,
                ) {
                    let node_transform =
                        parent_transform * uv::Mat4::from(node.transform().matrix());

                    ctx.node_transforms_world[node.index()] = node_transform;
                    if let (Some(mesh), Some(skin)) = (node.mesh(), node.skin()) {
                        ctx.mesh_skin_map.insert(mesh.index(), skin.index());
                    }

                    for child in node.children() {
                        traverse_node(ctx, child, node_transform);
                    }
                }
                traverse_node(
                    &mut TraversalContext {
                        node_transforms_world: &mut node_transforms_world,
                        mesh_skin_map: &mut mesh_skin_map,
                    },
                    node,
                    uv::Mat4::identity(),
                );
            }
        }

        // skins

        let loaded_skins: Vec<td::Index> = doc
            .skins()
            .map(|gltf_skin| {
                let Some(root_joint) = gltf_skin.joints().next() else {
                    eprintln!("Skin without joints");
                    return td::Index::DANGLING;
                };
                let root_transform = node_transforms_world[root_joint.index()];
                let loaded_skin = gltf_import::load_skin(&bufs, gltf_skin, root_transform);
                // evaluate the initial joint matrices in case this skin is used without animation
                loaded_skin.evaluate_joint_matrices();
                self.skins.insert(loaded_skin)
            })
            .collect();

        // animations

        for gltf_anim in doc.animations() {
            // find the skin containing the node associated with the first channel.
            // we'll assume all animation channels target nodes in the same skin
            let first_channel_target = gltf_anim.channels().next().map(|chan| chan.target().node());
            let Some(assoc_skin) = first_channel_target.and_then(|target| {
                doc.skins().find(|gltf_skin| {
                    gltf_skin
                        .joints()
                        .any(|joint| joint.index() == target.index())
                })
            }) else {
                continue;
            };

            let anim = gltf_import::load_animation(&bufs, assoc_skin.clone(), gltf_anim.clone());
            let anim_id = self.animations.insert(anim);

            self.anim_target_map
                .insert_at(anim_id, loaded_skins[assoc_skin.index()]);
            if let Some(name) = gltf_anim.name() {
                self.anim_name_map.insert(name_to_id(name), anim_id);
            }
        }

        // materials

        let loaded_materials: Vec<td::Index> = doc
            .materials()
            .map(|gltf_mat| {
                let mat_params = gltf_import::load_material(&images, gltf_mat);
                let mat = Material::new(rend, &self.material_res, mat_params);
                self.materials.insert(mat)
            })
            .collect();

        // meshes

        for gltf_mesh in doc.meshes() {
            for gltf_prim in gltf_mesh.primitives() {
                let mesh_data = gltf_import::load_mesh_data(&bufs, gltf_prim.clone());

                let mesh = MeshParams {
                    data: mesh_data,
                    ..Default::default()
                }
                .upload(&rend.device, gltf_mesh.name());

                let mesh_id = self.meshes.insert(mesh);
                if let Some(name) = gltf_mesh.name() {
                    self.mesh_name_map.insert(name_to_id(name), mesh_id);
                }
                if let Some(mat_idx) = gltf_prim.material().index() {
                    self.mesh_material_map
                        .insert_at(mesh_id, loaded_materials[mat_idx]);
                }
                if let Some(&skin_idx) = mesh_skin_map.get(&gltf_mesh.index()) {
                    self.mesh_skin_map
                        .insert_at(mesh_id, loaded_skins[skin_idx]);
                }
            }
        }

        Ok(())
    }

    /// Remove all loaded assets and state.
    ///
    /// This is a crude solution; ideally we'd like to load
    /// a common set of assets on game start that never gets unloaded
    /// and then only remove level-specific state on level change.
    /// Better garbage collection is being thought about
    /// and will be implemented later.
    pub fn clear(&mut self) {
        self.meshes.clear();
        self.mesh_name_map.clear();
        self.mesh_skin_map.clear();
        self.mesh_material_map.clear();
        self.skins.clear();
        self.animations.clear();
        self.anim_name_map.clear();
        self.anim_target_map.clear();
        self.animators.clear();
        self.materials.clear();
    }

    /// Add a mesh to the set of drawable assets.
    ///
    /// Returns a pre-resolved [`MeshId`] that can be used to render the mesh
    /// by inserting it into a [`hecs`] world.
    /// If `name` is given, the mesh can also be accessed by
    /// looking it up later with [`get_mesh_id`][Self::get_mesh_id].
    ///
    /// Note: this does not automatically associate a skin or material with the mesh.
    #[inline]
    pub fn insert_mesh(&mut self, mesh: Mesh, name: Option<&str>) -> MeshId {
        let key = self.meshes.insert(mesh);
        if let Some(id) = name {
            self.mesh_name_map.insert(id.to_string(), key);
        }
        MeshId {
            mesh: key,
            skin: None,
        }
    }

    #[inline]
    pub fn get_mesh_id(&self, name: &str) -> Option<MeshId> {
        self.mesh_name_map.get(name).map(|&mesh| MeshId {
            mesh,
            skin: self.mesh_skin_map.get(mesh).copied(),
        })
    }

    pub fn new_animation_target(&mut self, mesh_id: MeshId) -> MeshId {
        if let Some(skin) = mesh_id.skin.and_then(|skin_id| self.skins.get(skin_id)) {
            let new_skin = self.skins.insert(skin.clone());
            MeshId {
                skin: Some(new_skin),
                ..mesh_id
            }
        } else {
            mesh_id
        }
    }

    #[inline]
    pub fn get_mesh(&self, id: &MeshId) -> Option<&Mesh> {
        self.meshes.get(id.mesh)
    }

    #[inline]
    pub fn get_mesh_mut(&mut self, id: &MeshId) -> Option<&mut Mesh> {
        self.meshes.get_mut(id.mesh)
    }

    #[inline]
    pub fn get_mesh_material(&self, id: &MeshId) -> &Material {
        self.mesh_material_map
            .get(id.mesh)
            .and_then(|mat_idx| self.materials.get(*mat_idx))
            .unwrap_or(&self.default_material)
    }

    #[inline]
    pub fn get_animation_id(&self, name: &str) -> Option<AnimationId> {
        self.anim_name_map.get(name).copied().map(AnimationId)
    }

    /// Add an Animator that controls the playback of a single animation at a time.
    ///
    /// Returns an id that can be used to modify the playback state later.
    pub fn insert_animator(&mut self, anim: Animator) -> AnimatorId {
        AnimatorId(self.animators.insert(anim))
    }

    pub fn update_animations(&mut self, dt: f32) {
        for (_, animator) in self.animators.iter_mut() {
            let anim_id = animator.animation.0;
            let Some(animation) = self.animations.get(anim_id) else {
                continue;
            };
            animator.step_time(dt, animation);

            let target_skin_id =
                if let Some(t_override) = animator.target.as_ref().and_then(|t| t.skin) {
                    t_override
                } else if let Some(target) = self.anim_target_map.get(anim_id) {
                    *target
                } else {
                    continue;
                };
            let Some(target_skin) = self.skins.get_mut(target_skin_id) else {
                continue;
            };
            animator.update_skin(animation, target_skin);
            target_skin.update_global_poses();
        }
    }
}
