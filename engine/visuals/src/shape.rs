use graphics::math::Vec2d;
use graphics::types::Color;
use graphics::{Context, Graphics, Transformed};

use crate::Drawable;
use moleengine::ecs::system::*;
use moleengine::util::Transform;

#[derive(Clone)]
pub struct Shape {
    points: Vec<Vec2d<f64>>,
    color: Color,
    outline_radius: f64,
    outline_color: Color,
}

impl Shape {
    pub fn new(points: Vec<Vec2d<f64>>, color: Color) -> Self {
        Shape {
            points,
            color,
            outline_radius: 0.0,
            outline_color: [0.0; 4],
        }
    }

    pub fn new_outlined(
        points: Vec<Vec2d<f64>>,
        color: Color,
        outline_thickness: f64,
        outline_color: Color,
    ) -> Self {
        Shape {
            points,
            color,
            outline_radius: outline_thickness * 0.5,
            outline_color,
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
        if self.outline_radius > 0.0 {
            for i in 0..(self.points.len() - 1) {
                graphics::line(
                    self.outline_color,
                    self.outline_radius,
                    [
                        self.points[i][0],
                        self.points[i][1],
                        self.points[i + 1][0],
                        self.points[i + 1][1],
                    ],
                    ctx.transform,
                    gfx,
                );
            }
            graphics::line(
                self.outline_color,
                self.outline_radius,
                [
                    self.points.last().unwrap()[0],
                    self.points.last().unwrap()[1],
                    self.points[0][0],
                    self.points[0][1],
                ],
                ctx.transform,
                gfx,
            )
        }
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
            let ctx_ = self.ctx.append_transform(item.transform.for_gfx());
            item.shape.draw(&ctx_, self.gfx);
        }
    }
}
