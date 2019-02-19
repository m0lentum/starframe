use crate::Collision;
use graphics::{types::Color, Context, Graphics};
use moleengine::ecs::space::Space;
use nalgebra::Point2;

const COLL_INDICATOR_RADIUS: f32 = 5.0;

/// Draws a cross at the intersection points of each collision that happened in the previous physics update,
/// and a line representing the collision depth and normal direction.
pub fn draw_collisions<G: Graphics>(space: &Space, ctx: &Context, gfx: &mut G, color: Color) {
    space.do_with_global_state(|colls: &Vec<Collision>| {
        for coll in colls {
            let pen = -coll.depth * *coll.normal;
            coll.manifold.for_each(|point: &Point2<f32>| {
                graphics::line(
                    color,
                    0.5,
                    [
                        f64::from(point[0] - COLL_INDICATOR_RADIUS),
                        f64::from(point[1] - COLL_INDICATOR_RADIUS),
                        f64::from(point[0] + COLL_INDICATOR_RADIUS),
                        f64::from(point[1] + COLL_INDICATOR_RADIUS),
                    ],
                    ctx.transform,
                    gfx,
                );
                graphics::line(
                    color,
                    0.5,
                    [
                        f64::from(point[0] - COLL_INDICATOR_RADIUS),
                        f64::from(point[1] + COLL_INDICATOR_RADIUS),
                        f64::from(point[0] + COLL_INDICATOR_RADIUS),
                        f64::from(point[1] - COLL_INDICATOR_RADIUS),
                    ],
                    ctx.transform,
                    gfx,
                );

                let pen_point = point + pen;
                graphics::line(
                    color,
                    0.5,
                    [
                        f64::from(point[0]),
                        f64::from(point[1]),
                        f64::from(pen_point[0]),
                        f64::from(pen_point[1]),
                    ],
                    ctx.transform,
                    gfx,
                );
            });
        }
    });
}
