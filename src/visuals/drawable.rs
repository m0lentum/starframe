use graphics::{Context, Graphics};

pub trait Drawable {
    fn draw<G: Graphics>(&mut self, ctx: &Context, gfx: &mut G);
}
