use super::{shaders::Shaders, Color, Vertex2D};
use crate::ecs::system::*;
use crate::util::Transform;

use glium::{backend::Facade, uniform, Surface};
use std::sync::Arc;

#[derive(Clone)]
pub struct Shape {
    pub(self) verts: Arc<glium::VertexBuffer<Vertex2D>>,
    pub(self) color: Color,
}

impl Shape {
    pub fn new<F: Facade + ?Sized>(facade: &F, points: &[[f32; 2]], color: Color) -> Self {
        let points_as_verts: Vec<Vertex2D> =
            points.iter().map(|p| Vertex2D { v_position: *p }).collect();
        Shape {
            verts: Arc::new(
                glium::VertexBuffer::new(facade, points_as_verts.as_slice())
                    .expect("Failed to create vertex buffer"),
            ),
            color,
        }
    }

    pub fn new_square<F: Facade + ?Sized>(facade: &F, width: f32, color: Color) -> Self {
        let hw = width / 2.0;
        Shape::new(facade, &[[-hw, -hw], [hw, -hw], [hw, hw], [-hw, hw]], color)
    }
}

/// System that draws Shapes on the screen.
/// A Transform must also be present for the Shape to be drawn.
/// See the moleengine_ecs crate for more information on Systems.
pub struct ShapeRenderer<'a, S: Surface> {
    pub target: &'a mut S,
    pub shaders: &'a Shaders,
}

/// The component filter for ShapeRenderer.
#[derive(ComponentFilter)]
pub struct ShapeFilter<'a> {
    transform: &'a Transform,
    shape: &'a Shape,
}

impl<'a, S: Surface> SimpleSystem<'a> for ShapeRenderer<'a, S> {
    type Filter = ShapeFilter<'a>;

    fn run_system(self, items: &mut [Self::Filter]) {
        // TODO dynamic view (must also adapt to changing window size)
        let view =
            nalgebra::Matrix3::new(2.0 / 800.0, 0.0, 0.0, 0.0, 2.0 / 600.0, 0.0, 0.0, 0.0, 1.0);

        for item in items {
            let model = item.transform.to_homogeneous();
            let mv: [[f32; 3]; 3] = (view * model).into();
            let uniforms = glium::uniform! {
                model_view: mv,
                color: item.shape.color,
            };
            self.target
                .draw(
                    &*item.shape.verts,
                    glium::index::NoIndices(glium::index::PrimitiveType::TriangleFan),
                    &self.shaders.ortho_2d,
                    &uniforms,
                    &Default::default(),
                )
                .expect("Drawing failed");
        }
    }
}

// not sure if I want to use this or let the user set number of verts
//
//lazy_static::lazy_static! {
//    /// All circles are the same so we can precalculate their vertices
//    static ref CIRCLE_POINTS: Vec<Vector2<f32>> = {
//        let angle_incr = 2.0 * std::f32::consts::PI / COLLIDER_MAX_VERTS as f32;
//        (0..COLLIDER_MAX_VERTS).map(|i| {
//            let angle = angle_incr * i as f32;
//            Vector2::new(angle.cos(), angle.sin())
//        }).collect()
//    };
//}
