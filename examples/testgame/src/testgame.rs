use moleengine_core::game::*;

use crate::states::{Data, Playing};

use glutin_window::GlutinWindow;
use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::*;
use piston::window::WindowSettings;

pub fn launch() {
    let opengl = OpenGL::V3_2;
    // TODO: these settings should be taken from config
    // also should allow recreating window mid-run to change settings
    let mut window: GlutinWindow = WindowSettings::new("MoleEngine Test", [800, 600])
        .opengl(opengl)
        .vsync(false)
        .exit_on_esc(true)
        .build()
        .unwrap();
    let gl = GlGraphics::new(opengl);
    let mut events = Events::new(EventSettings::new().ups(120).max_fps(240));

    let mut game = Game::new(Data::init(gl), Box::new(Playing));

    // game loop
    while let Some(evt) = events.next(&mut window) {
        game.on_event(&evt);
    }
}
