extern crate glutin_window;
extern crate graphics;
extern crate moleengine_core;
extern crate moleengine_ecs;
extern crate moleengine_visuals;
extern crate opengl_graphics;
extern crate piston;

mod states;
mod testgame;

fn main() {
    testgame::launch();
}
