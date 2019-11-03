use crate::{
    physics2d::collision::ContactOutput,
    visuals_glium::{
        camera::{Camera2D, CameraController},
        Color, Shaders, Vertex2D,
    },
};
use glium::{backend::Facade, uniform};

const COLL_INDICATOR_SIZE: f32 = 5.0;
const VERTS_PER_INDICATOR: usize = 6;

/// A System that draws indicators showing the location and depth of collisions.
/// Should be run after collision detection.
/// This needs persistent state because it holds a vertex buffer for rendering.
pub struct ContactIndicator {
    vb: glium::VertexBuffer<Vertex2D>,
}

impl ContactIndicator {
    /// Reserves a vertex buffer with room for `capacity` indicators
    /// and returns a new ContactIndicator containing it.
    pub fn new<F: Facade + ?Sized>(facade: &F, capacity: usize) -> Self {
        ContactIndicator {
            vb: glium::VertexBuffer::empty_dynamic(facade, capacity * VERTS_PER_INDICATOR)
                .expect("Failed to create vertex buffer"),
        }
    }

    /// Draws an indicator on all given collisions, used for debugging purposes.
    pub fn draw<C: CameraController, S: glium::Surface>(
        &mut self,
        camera: &Camera2D<C>,
        target: &mut S,
        contacts: &ContactOutput,
        color: Color,
        shaders: &Shaders,
    ) {
        // update vertex buffer

        for (coll, verts) in contacts
            .0
            .iter()
            .zip(self.vb.map().chunks_mut(VERTS_PER_INDICATOR))
        {
            let normal_scaled = *coll.normal * COLL_INDICATOR_SIZE;
            let tangent_scaled = nalgebra::Vector2::new(normal_scaled[1], -normal_scaled[0]);
            verts[0] = (coll.point + normal_scaled + tangent_scaled).into();
            verts[1] = (coll.point - normal_scaled - tangent_scaled).into();
            verts[2] = (coll.point + normal_scaled - tangent_scaled).into();
            verts[3] = (coll.point - normal_scaled + tangent_scaled).into();
            verts[4] = coll.point.into();
            verts[5] = (coll.point + (*coll.normal * coll.depth)).into();
        }

        // draw

        let view: [[f32; 3]; 3] = camera.view_matrix().into();
        let uniforms = glium::uniform! {
            model_view: view,
            color: color,
        };

        target
            .draw(
                self.vb
                    .slice(..(contacts.0.len() * VERTS_PER_INDICATOR).min(self.vb.len()))
                    .expect("Range error on ContactIndicator vertex buffer"),
                glium::index::NoIndices(glium::index::PrimitiveType::LinesList),
                &shaders.ortho_2d,
                &uniforms,
                &Default::default(),
            )
            .expect("ContactIndicator drawing failed");
    }
}
