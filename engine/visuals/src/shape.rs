use graphics::math::Vec2d;
use graphics::types::Color;
use graphics::{Context, Graphics};

use crate::Drawable;
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
/// See the moleengine_ecs crate for more information on Systems.
pub struct ShapeRenderer<'a, G: Graphics> {
    ctx: &'a Context,
    gfx: &'a mut G,
}

impl<'a, G: Graphics> ShapeRenderer<'a, G> {
    /// Create a ShapeRenderer from the given references.
    /// Named `context` instead of `new` to emphasize that these objects are
    /// only used as consumed contexts when running the system.
    pub fn context(ctx: &'a Context, gfx: &'a mut G) -> Self {
        ShapeRenderer { ctx, gfx }
    }
}

/// The component filter for ShapeRenderer.
#[derive(ComponentFilter)]
pub struct ShapeFilter<'a> {
    shape: &'a Shape,
}

impl<'a, G: Graphics> SimpleSystem<'a> for ShapeRenderer<'a, G> {
    type Filter = ShapeFilter<'a>;
    fn run_system(self, items: &mut [Self::Filter]) {
        for item in items {
            item.shape.draw(self.ctx, self.gfx);
        }
    }
}
