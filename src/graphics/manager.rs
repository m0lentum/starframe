use std::collections::HashMap;
use thunderdome as td;

use super::{
    material::{Material, MaterialDefaults},
    mesh::{Mesh, MeshData},
    Renderer, Skin,
};
use crate::math::uv;

#[cfg(feature = "gltf")]
mod gltf_import;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AssetId {
    Unresolved(String),
    Resolved(td::Index),
}

#[derive(Default, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AssetIdWithDefault {
    #[default]
    Default,
    Loaded(AssetId),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MeshId(AssetId);

impl<S> From<S> for MeshId
where
    String: From<S>,
{
    fn from(value: S) -> Self {
        Self(AssetId::Unresolved(String::from(value)))
    }
}

#[derive(Default, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MaterialId(AssetIdWithDefault);

impl From<td::Index> for MaterialId {
    fn from(value: td::Index) -> Self {
        Self(AssetIdWithDefault::Loaded(AssetId::Resolved(value)))
    }
}

pub struct GraphicsManager {
    meshes: td::Arena<Mesh>,
    /// map from mesh names to mesh ids
    mesh_name_map: HashMap<String, td::Index>,
    /// map from mesh ids to skin ids
    mesh_skin_map: td::Arena<td::Index>,
    skins: td::Arena<Skin>,
    materials: td::Arena<Material>,
    mat_defaults: MaterialDefaults,
    // TODO: animations
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
        Self {
            meshes: td::Arena::new(),
            mesh_name_map: HashMap::new(),
            mesh_skin_map: td::Arena::new(),
            skins: td::Arena::new(),
            materials: td::Arena::new(),
            mat_defaults: MaterialDefaults::new(rend),
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
                        ctx.mesh_skin_map[&mesh.index()] = skin.index();
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
                self.skins.insert(loaded_skin)
            })
            .collect();

        // materials

        let loaded_materials: Vec<MaterialId> = doc
            .materials()
            .map(|gltf_mat| {
                let mat_params = gltf_import::load_material(&images, gltf_mat);
                let mat = Material::new(rend, &self.mat_defaults, mat_params);
                let id = self.materials.insert(mat);
                MaterialId::from(id)
            })
            .collect();

        // meshes

        for gltf_mesh in doc.meshes() {
            let primitives: Vec<_> = gltf_mesh
                .primitives()
                .map(|gltf_prim| {
                    let mut prim = gltf_import::load_mesh_primitive(&bufs, gltf_prim);
                    if let Some(mat_idx) = gltf_prim.material().index() {
                        prim.material = loaded_materials[mat_idx];
                    }
                    prim
                })
                .collect();

            // this assert is ugly, would be nice to express
            // the skinned/unskinned distinction at the mesh type level,
            // but it's a bit tricky to do that ergonomically.
            // TODO: maybe only support one primitive per mesh?
            // or make different primitives different meshes on the staframe side,
            // since they're rendered as if they're separate meshes anyway?
            assert!(
                primitives.iter().all(|prim| prim.joints.is_some())
                    || primitives.iter().all(|prim| prim.joints.is_none()),
                "Mixing unskinned and skinned mesh primitives is not supported"
            );

            let mesh = MeshData {
                label: gltf_mesh.name(),
                primitives: &primitives,
                ..Default::default()
            }
            .upload(&rend.device);

            let mesh_id = self.meshes.insert(mesh);
            if let Some(name) = gltf_mesh.name() {
                self.mesh_name_map.insert(name_to_id(name), mesh_id);
            }
            if let Some(&skin_idx) = mesh_skin_map.get(&gltf_mesh.index()) {
                self.mesh_skin_map
                    .insert_at(mesh_id, loaded_skins[skin_idx]);
            }
        }

        Ok(())
    }

    /// Add a mesh to the set of drawable assets.
    ///
    /// Returns a pre-resolved [`MeshId`] that can be used to render the mesh
    /// by inserting it into a [`hecs`] world.
    /// If `name` is given, the mesh can also be accessed by
    /// creating a [`MeshId`] with the same string.
    pub fn insert_mesh(&mut self, mesh: Mesh, name: Option<String>) -> MeshId {
        let key = self.meshes.insert(mesh);
        if let Some(id) = name {
            self.mesh_name_map.insert(id, key);
        }
        MeshId(AssetId::Resolved(key))
    }
}
