use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::*;
use piston::input::Key;
use piston::input::*;
use piston_window::{PistonWindow, WindowSettings};

use moleengine::{
    ecs::{
        recipe::{ObjectRecipe, RecipeBook},
        space::Space,
        storage::VecStorage,
    },
    util::{inputstate::*, Transform},
};

use moleengine_visuals::shape::*;

use moleengine_physics::{systems::Motion, RigidBody};

const BG_COLOR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
const LINE_COLOR: [f32; 4] = [0.729, 0.855, 0.333, 1.0];

pub struct Resources {
    pub gl: GlGraphics,
    pub window: PistonWindow,
    pub events: Events,
    pub input_state: InputState,
    pub recipes: RecipeBook,
    pub space: Space,
}

pub fn init() -> Resources {
    let opengl = OpenGL::V3_2;
    let window: PistonWindow = WindowSettings::new("MoleEngine game template", [800, 600])
        .opengl(opengl)
        .vsync(false)
        .exit_on_esc(true)
        .build()
        .unwrap();
    let gl = GlGraphics::new(opengl);
    let events = Events::new(EventSettings::new().ups(60).max_fps(120));

    let mut input_state = InputState::new();
    {
        use Key::*;
        input_state.track_keys(&[Up, Down, Left, Right]);
    }

    let mut space = Space::with_capacity(100);
    space
        .add_container::<Transform, VecStorage<_>>()
        .add_container::<RigidBody, VecStorage<_>>()
        .add_container::<Shape, VecStorage<_>>();

    let mut recipes = RecipeBook::new();
    let mut block = ObjectRecipe::new()
        .add_variable(Some(Transform::from_position([150.0, 150.0])))
        .add({
            let mut rb = RigidBody::new();
            rb.angular_vel = 0.03;
            rb
        })
        .add(Shape::new_square(80.0, [1.0; 4]));
    block.apply(&mut space);
    block.set_variable(Transform::from_position([300.0, 300.0]));
    block.apply(&mut space);
    block.reset_variables();
    recipes.add("block", block);

    Resources {
        gl,
        window,
        events,
        input_state,
        recipes,
        space,
    }
}

pub fn begin(mut res: Resources) {
    state_playing(&mut res);
}

fn state_playing(res: &mut Resources) {
    while let Some(evt) = res.events.next(&mut res.window) {
        res.input_state.handle_event(&evt);

        if let Some(args) = evt.render_args() {
            draw_space(&mut res.gl, args, &mut res.space);
        } else if let Some(_args) = evt.update_args() {
            update_space(&mut res.space);
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

fn draw_space(gl: &mut GlGraphics, args: RenderArgs, space: &mut Space) {
    let ctx = gl.draw_begin(args.viewport());

    graphics::clear(BG_COLOR, gl);

    space.run_system(ShapeRenderer::new(&ctx, gl));

    gl.draw_end();
}

fn update_space(space: &mut Space) {
    space.run_system(Motion);
}
