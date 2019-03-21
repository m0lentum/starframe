use piston::event_loop::*;
use piston::input::Key;
use piston::input::*;
use piston_window::{OpenGL, PistonWindow, WindowSettings};

use moleengine::util::inputstate::*;

use crate::fluidbox::FluidBox;

const BG_COLOR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];

pub struct Resources {
    pub window: PistonWindow,
    pub events: Events,
    pub input_state: InputState,
    pub fluid: FluidBox,
}

pub fn init() -> Resources {
    let opengl = OpenGL::V3_2;
    let window: PistonWindow = WindowSettings::new("MoleEngine game template", [500, 500])
        .opengl(opengl)
        .vsync(false)
        .exit_on_esc(true)
        .build()
        .unwrap();
    let events = Events::new(EventSettings::new().ups(60).max_fps(60));

    let mut input_state = InputState::new();
    {
        use Key::*;
        input_state.track_keys(&[Up, Down, Left, Right]);
    }

    let fluid = FluidBox::new(50, 50, 10.0);

    Resources {
        window,
        events,
        input_state,
        fluid,
    }
}

pub fn begin(mut res: Resources) {
    state_playing(&mut res);
}

fn state_playing(res: &mut Resources) {
    while let Some(evt) = res.window.next() {
        res.input_state.handle_event(&evt);

        if let Some(_args) = evt.render_args() {
            res.window.draw_2d(&evt, |_ctx, gfx| {
                graphics::clear(BG_COLOR, gfx);
            });
            res.fluid.draw_density(&evt, &mut res.window);
            res.fluid.draw_velocity(&evt, &mut res.window);
        } else if let Some(_args) = evt.update_args() {
            res.fluid.add_source_at(5, 5, 0.2);
            res.input_state.update_ages();
        } else if let Some(Button::Keyboard(Key::Space)) = evt.press_args() {
            state_paused(res);
        }
    }
}

fn state_paused(res: &mut Resources) {
    while let Some(evt) = res.events.next(&mut res.window) {
        if let Some(Button::Keyboard(Key::Space)) = evt.press_args() {
            return;
        }
    }
}
