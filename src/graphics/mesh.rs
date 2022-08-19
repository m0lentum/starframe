use crate::{graph, graphics as gx, math as m, physics as phys};

mod batched;
pub use batched::{BatchedMesh, ConvexMeshShape};

mod skinned;
pub use skinned::SkinnedMesh;

#[cfg(feature = "gltf")]
mod gltf_import;

/// A triangle mesh for rendering. Can be animated with a skin
/// and imported from glTF documents (with the `gltf` crate feature enabled).
#[derive(Debug)]
pub struct Mesh {
    pub offset: m::Pose,
    pub kind: MeshKind,
}

#[derive(Debug)]
pub enum MeshKind {
    /// Mesh with a skin and animations attached to it.
    Skinned(skinned::SkinnedMesh),
    /// Mesh with no skin or animations that is drawn in one draw call
    /// with all other SimpleBatched meshes in the world.
    SimpleBatched(batched::BatchedMesh),
}

impl Mesh {
    #[cfg(feature = "gltf")]
    #[inline]
    pub fn from_gltf(rend: &gx::Renderer, doc: &gltf::Document, buffers: &[&[u8]]) -> Self {
        gltf_import::import_mesh(rend, doc, buffers)
    }

    #[inline]
    pub fn from_collider_shape(shape: &phys::ColliderShape, max_circle_vert_distance: f64) -> Self {
        Self {
            offset: m::Pose::identity(),
            kind: MeshKind::SimpleBatched(batched::BatchedMesh::from_collider_shape(
                shape,
                max_circle_vert_distance,
            )),
        }
    }

    /// Set the offset of the mesh from the pose it's attached to.
    #[inline]
    pub fn with_offset(mut self, offset: m::Pose) -> Self {
        self.offset = offset;
        self
    }

    /// Overwrite the color of every vertex with a new one.
    /// Does not do anything on skinned meshes.
    #[inline]
    pub fn with_color(mut self, color: [f32; 4]) -> Self {
        match &mut self.kind {
            MeshKind::SimpleBatched(b) => {
                for vert in &mut b.vertices {
                    vert.color = color;
                }
            }
            _ => {}
        }
        self
    }

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

impl From<BatchedMesh> for Mesh {
    fn from(m: BatchedMesh) -> Self {
        Self {
            offset: m::Pose::identity(),
            kind: MeshKind::SimpleBatched(m),
        }
    }
}

impl From<SkinnedMesh> for Mesh {
    fn from(m: SkinnedMesh) -> Self {
        Self {
            offset: m::Pose::identity(),
            kind: MeshKind::Skinned(m),
        }
    }
}

impl From<phys::Collider> for Mesh {
    fn from(coll: phys::Collider) -> Self {
        BatchedMesh::from(coll).into()
    }
}

impl From<ConvexMeshShape> for Mesh {
    fn from(shape: ConvexMeshShape) -> Self {
        BatchedMesh::from(shape).into()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum AnimationError {
    NotAnimated,
    FeatureNotFound,
}

/// Renderer that can draw any kind of mesh, skinned or not.
pub struct MeshRenderer {
    batched: batched::Renderer,
    skinned: skinned::Renderer,
}

impl MeshRenderer {
    pub fn new(rend: &gx::Renderer) -> Self {
        Self {
            batched: batched::Renderer::new(rend),
            skinned: skinned::Renderer::new(rend),
        }
    }

    /// Step all skin animations forward in time by `dt`.
    pub fn step_time(&mut self, dt: f32, l_mesh: graph::LayerViewMut<super::Mesh>) {
        self.skinned.step_time(dt, l_mesh);
    }

    /// Draw all meshes to the screen.
    pub fn draw(
        &mut self,
        camera: &gx::Camera,
        ctx: &mut gx::RenderContext,
        (mut l_mesh, l_pose): (graph::LayerViewMut<Mesh>, graph::LayerView<m::Pose>),
    ) {
        self.batched
            .draw(camera, ctx, (l_mesh.subview(), l_pose.subview()));
        self.skinned
            .draw(camera, ctx, (l_mesh.subview_mut(), l_pose.subview()));
    }
}
