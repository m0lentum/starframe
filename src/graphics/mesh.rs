use crate::{graph, graphics as gx, math as m, physics as phys};

mod batched;
pub use batched::{BatchedMesh, ConvexMeshShape};

mod skinned;
pub use skinned::SkinnedMesh;

pub(crate) mod skin;
pub use skin::Skin;

#[cfg(feature = "gltf")]
pub mod gltf_import;

/// A triangle mesh for rendering. Can be animated with a skin
/// and imported from glTF documents (with the `gltf` crate feature enabled).
#[derive(Debug)]
pub struct Mesh {
    pub offset: m::Pose,
    pub has_outline: bool,
    pub kind: MeshKind,
}

#[derive(Debug)]
pub enum MeshKind {
    /// Mesh with a skin and animations attached to it.
    ///
    /// Boxed because it's much larger than the unskinned variant.
    Skinned(Box<skinned::SkinnedMesh>),
    /// Mesh with no skin or animations that is drawn in one draw call
    /// with all other SimpleBatched meshes in the world.
    SimpleBatched(batched::BatchedMesh),
}

impl Mesh {
    #[cfg(feature = "gltf")]
    #[inline]
    pub fn from_gltf(rend: &gx::Renderer, doc: &gltf::Document, buffers: &[&[u8]]) -> Self {
        gltf_import::load_mesh(rend, doc, buffers)
    }

    #[inline]
    pub fn from_collider_shape(shape: &phys::ColliderShape, max_circle_vert_distance: f64) -> Self {
        batched::BatchedMesh::from_collider_shape(shape, max_circle_vert_distance).into()
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
        if let MeshKind::SimpleBatched(b) = &mut self.kind {
            for vert in &mut b.vertices {
                vert.color = color;
            }
        }
        self
    }

    /// Opt out of drawing an outline for this mesh when
    /// [`OutlineRenderer`][super::OutlineRenderer] is run.
    #[inline]
    pub fn without_outline(mut self) -> Self {
        self.has_outline = false;
        self
    }
}

impl From<BatchedMesh> for Mesh {
    fn from(m: BatchedMesh) -> Self {
        Self {
            offset: m::Pose::identity(),
            has_outline: true,
            kind: MeshKind::SimpleBatched(m),
        }
    }
}

impl From<SkinnedMesh> for Mesh {
    fn from(m: SkinnedMesh) -> Self {
        Self {
            offset: m::Pose::identity(),
            has_outline: true,
            kind: MeshKind::Skinned(Box::new(m)),
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

    /// Draw all meshes to the screen.
    pub fn draw(
        &mut self,
        camera: &gx::Camera,
        ctx: &mut gx::RenderContext,
        (mut l_mesh, l_skin, l_pose): (
            graph::LayerViewMut<Mesh>,
            graph::LayerView<Skin>,
            graph::LayerView<m::Pose>,
        ),
    ) {
        self.batched
            .draw(camera, ctx, (l_mesh.subview(), l_pose.subview()));
        self.skinned.draw(
            camera,
            ctx,
            (l_mesh.subview_mut(), l_skin.subview(), l_pose.subview()),
        );
    }
}
