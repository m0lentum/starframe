use graphics::{Context, Graphics};

pub mod shape;

/// Things that can draw themselves into a rendering context.
pub trait Drawable {
    fn draw<G: Graphics>(&self, ctx: &Context, gfx: &mut G);
}
