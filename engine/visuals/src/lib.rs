use graphics::{Context, Graphics};

pub mod shape;

pub trait Drawable {
    fn draw<G: Graphics>(&self, ctx: &Context, gfx: &mut G);
}
