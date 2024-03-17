use thunderdome as td;

use super::{Mesh, Skin};

#[cfg(feature = "gltf")]
pub(crate) mod gltf_import;

#[derive(Default)]
pub struct GraphicsManager {
    meshes: td::Arena<Mesh>,
    skins: td::Arena<Skin>,
    // TODO: materials, animations
}

/// Error when loading assets from a glTF document.
#[cfg(feature = "gltf")]
#[derive(thiserror::Error, Debug)]
pub enum LoadError {
    #[error("Failed to open the file at the given path")]
    IoError(#[from] std::io::Error),
    #[error("Failed to read glTF document")]
    GltfError(#[from] gltf::Error),
}

impl GraphicsManager {
    /// Load all graphics assets (meshes, skins, materials, animations)
    /// in a glTF document given as a byte slice.
    #[cfg(feature = "gltf")]
    pub fn load_gltf(&mut self, gltf_bytes: &[u8]) -> Result<(), gltf::Error> {
        let (doc, bufs, images) = gltf::import_slice(gltf_bytes)?;
        let bufs: Vec<&[u8]> = bufs.iter().map(|data| data.0.as_slice()).collect();

        for mesh in gltf_import::load_meshes(&doc, &bufs) {
            // TODO: mesh id and map from ids to arena handles
            self.meshes.insert(mesh);
        }

        for skin in gltf_import::load_skins(&doc, &bufs) {
            self.skins.insert(skin);
        }

        Ok(())
    }
}
