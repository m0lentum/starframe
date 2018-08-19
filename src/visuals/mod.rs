use graphics::{Context, Graphics};

pub use self::shape::Shape;

mod shape;

pub trait Drawable {
    fn draw<G: Graphics>(&mut self, ctx: &Context, gfx: &mut G);
}
