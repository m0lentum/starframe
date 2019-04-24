use super::{shaders::Shaders, Color, Vertex2D};
use crate::ecs::system::*;
use crate::util::Transform;

use glium::{backend::Facade, uniform, Surface};
use std::sync::Arc;

/// A flat-colored convex polygon shape, rendered using the ShapeRenderer system.
/// When creating multiple identical shapes, it is preferable to create one and clone it,
/// as this reuses the same vertex buffer for all clones.
/// Concavity will not result in an error but will be rendered incorrectly.
#[derive(Clone)]
pub struct Shape {
    pub(self) verts: Arc<glium::VertexBuffer<Vertex2D>>,
    pub(self) color: Color,
}

impl Shape {
    /// Create a new Shape from a set of points.
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

    /// Create an axis-aligned square Shape with the given side length.
    pub fn new_square<F: Facade + ?Sized>(facade: &F, width: f32, color: Color) -> Self {
        let hw = width * 0.5;
        Self::new(facade, &[[-hw, -hw], [hw, -hw], [hw, hw], [-hw, hw]], color)
    }

    /// Create an axis-aligned rectangle Shape with the given dimensions.
    pub fn new_rect<F: Facade + ?Sized>(facade: &F, width: f32, height: f32, color: Color) -> Self {
        let hw = width * 0.5;
        let hh = height * 0.5;
        Self::new(facade, &[[-hw, -hh], [hw, -hh], [hw, hh], [-hw, hh]], color)
    }

    /// Create a polygonal approximation of a circle with the given radius and number of points.
    pub fn new_circle<F: Facade + ?Sized>(
        facade: &F,
        radius: f32,
        point_count: u32,
        color: Color,
    ) -> Self {
        let angle_incr = 2.0 * std::f32::consts::PI / point_count as f32;
        let pts: Vec<[f32; 2]> = (0..point_count)
            .map(|i| {
                let angle = angle_incr * i as f32;
                [radius * angle.cos(), radius * angle.sin()]
            })
            .collect();
        Self::new(facade, pts.as_slice(), color)
    }

    /// Create a Shape that matches the given Collider.
    /// Circle colliders are approximated with a polygon.
    #[cfg(feature = "physics2d")]
    pub fn from_collider<F: Facade + ?Sized>(
        facade: &F,
        coll: &crate::physics2d::Collider,
        color: Color,
    ) -> Self {
        use crate::physics2d::Collider;
        match coll {
            Collider::Circle { r } => {
                let pts: Vec<[f32; 2]> =
                    CIRCLE_VERTS.iter().map(|p| [r * p[0], r * p[1]]).collect();
                Self::new(facade, pts.as_slice(), color)
            }
            Collider::Rect { hw, hh } => Self::new(
                facade,
                &[[-hw, -hh], [*hw, -hh], [*hw, *hh], [-hw, *hh]],
                color,
            ),
        }
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

const CIRCLE_VERTS_COUNT: u32 = 16;

lazy_static::lazy_static! {
    /// All circles are the same so we can precalculate their vertices
    static ref CIRCLE_VERTS: Vec<[f32; 2]> = {
        let angle_incr = 2.0 * std::f32::consts::PI / CIRCLE_VERTS_COUNT as f32;
        (0..CIRCLE_VERTS_COUNT).map(|i| {
            let angle = angle_incr * i as f32;
            [angle.cos(), angle.sin()]
        }).collect()
    };
}
