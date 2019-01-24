use graphics::math::Vec2d;
use graphics::types::Color;
use graphics::{Context, Graphics, Transformed};

use crate::Drawable;
use moleengine_core::transform::*;
use moleengine_ecs::system::*;

#[derive(Clone)]
pub struct Shape {
    points: Vec<Vec2d<f64>>,
    color: Color,
    _outline_thickness: f64, // unimplemented
    _outline_color: Color,   // unimplemented
}

impl Shape {
    pub fn new(points: Vec<Vec2d<f64>>, color: Color) -> Self {
        Shape {
            points: points,
            color: color,
            _outline_thickness: 0.0,
            _outline_color: [0.0, 0.0, 0.0, 0.0],
        }
    }

    pub fn new_square(width: f64, color: Color) -> Self {
        let hw = width / 2.0;
        Shape::new(vec![[-hw, -hw], [hw, -hw], [hw, hw], [-hw, hw]], color)
    }
}

impl Drawable for Shape {
    fn draw<G: Graphics>(&self, ctx: &Context, gfx: &mut G) {
        graphics::polygon(self.color, &self.points, ctx.transform, gfx);
    }
}

/// System that draws Shapes on the screen.
/// A Transform must also be present for the Shape to be drawn.
/// See the moleengine_ecs crate for more information on Systems.
pub struct ShapeRenderer<'a, G: Graphics> {
    ctx: &'a Context,
    gfx: &'a mut G,
}

impl<'a, G: Graphics> ShapeRenderer<'a, G> {
    /// Create a ShapeRenderer from the given references.
    pub fn new(ctx: &'a Context, gfx: &'a mut G) -> Self {
        ShapeRenderer { ctx, gfx }
    }
}

/// The component filter for ShapeRenderer.
#[derive(ComponentFilter)]
pub struct ShapeFilter<'a> {
    transform: &'a Transform,
    shape: &'a Shape,
}

impl<'a, G: Graphics> SimpleSystem<'a> for ShapeRenderer<'a, G> {
    type Filter = ShapeFilter<'a>;

    fn run_system(self, items: &mut [Self::Filter]) {
        for item in items {
            let ctx_ = self.ctx.append_transform(transform_for_gfx(item.transform));
            item.shape.draw(&ctx_, self.gfx);
        }
    }
}
