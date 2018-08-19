use graphics::math::Vec2d;
use graphics::polygon;
use graphics::types::Color;
use graphics::{Context, Graphics};

use visuals::Drawable;

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
    fn draw<G: Graphics>(&mut self, ctx: &Context, gfx: &mut G) {
        polygon(self.color, &self.points, ctx.transform, gfx);
    }
}
