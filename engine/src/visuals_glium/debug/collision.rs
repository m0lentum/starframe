use crate::{
    ecs::space::Space,
    physics2d::Collision,
    visuals_glium::{Color, Shaders, Vertex2D},
};
use glium::{backend::Facade, uniform};

const COLL_INDICATOR_SIZE: f32 = 5.0;
const VERTS_PER_INDICATOR: usize = 6;

/// A System that draws indicators showing the location and depth of collisions.
/// Should be run after collision detection.
/// This needs persistent state because it holds a vertex buffer for rendering.
pub struct IntersectionIndicator {
    vb: glium::VertexBuffer<Vertex2D>,
}

impl IntersectionIndicator {
    /// Reserves a vertex buffer with room for `capacity` indicators
    /// and returns a new IntersectionIndicator containing it.
    pub fn new<F: Facade + ?Sized>(facade: &F, capacity: usize) -> Self {
        IntersectionIndicator {
            vb: glium::VertexBuffer::empty_dynamic(facade, capacity * VERTS_PER_INDICATOR)
                .expect("Failed to create vertex buffer"),
        }
    }

    pub fn draw_space<S: glium::Surface>(
        &mut self,
        target: &mut S,
        space: &Space,
        color: Color,
        shaders: &Shaders,
    ) {
        space.do_with_global_state(|colls: &Vec<Collision>| {
            // update vertex buffer

            for (coll, verts) in colls
                .iter()
                .zip(self.vb.map().chunks_mut(VERTS_PER_INDICATOR))
            {
                let center = coll.manifold.center();
                let normal_scaled = *coll.normal * COLL_INDICATOR_SIZE;
                let p1 = &coll.manifold.0;
                if let Some(p2) = coll.manifold.1 {
                    verts[0] = (p1 + normal_scaled).into();
                    verts[1] = (p1 - normal_scaled).into();
                    verts[2] = (p2 + normal_scaled).into();
                    verts[3] = (p2 - normal_scaled).into();
                } else {
                    let tangent_scaled =
                        nalgebra::Vector2::new(normal_scaled[1], -normal_scaled[0]);
                    verts[0] = (p1 + normal_scaled + tangent_scaled).into();
                    verts[1] = (p1 - normal_scaled - tangent_scaled).into();
                    verts[2] = (p1 + normal_scaled - tangent_scaled).into();
                    verts[3] = (p1 - normal_scaled + tangent_scaled).into();
                }
                verts[4] = center.into();
                verts[5] = (center - (*coll.normal * coll.depth)).into();
            }

            // draw

            let view: [[f32; 3]; 3] =
                nalgebra::Matrix3::new(2.0 / 800.0, 0.0, 0.0, 0.0, 2.0 / 600.0, 0.0, 0.0, 0.0, 1.0)
                    .into();
            let uniforms = glium::uniform! {
                model_view: view,
                color: color,
            };

            target
                .draw(
                    self.vb
                        .slice(..(colls.len() * VERTS_PER_INDICATOR).min(self.vb.len()))
                        .expect("Range error on IntersectionIndicator vertex buffer"),
                    glium::index::NoIndices(glium::index::PrimitiveType::LinesList),
                    &shaders.ortho_2d,
                    &uniforms,
                    &Default::default(),
                )
                .expect("IntersectionIndicator drawing failed");
        });
    }
}
