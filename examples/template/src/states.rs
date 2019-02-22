use moleengine::{
    ecs::{
        recipe::{ObjectRecipe, RecipeBook},
        space::Space,
        storage::VecStorage,
    },
    game::GameState,
    util::{inputstate::*, Transform},
};

use moleengine_visuals::shape::*;

use moleengine_physics::{systems::Motion, RigidBody};

use opengl_graphics::GlGraphics;
use piston::input::*;
use piston::input::{keyboard::Key, Button};

const BG_COLOR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
const LINE_COLOR: [f32; 4] = [0.729, 0.855, 0.333, 1.0];

pub struct Data {
    input_state: InputState,
    recipes: RecipeBook,
    gl: GlGraphics,
    space: Space,
}

impl Data {
    pub fn init(gl: GlGraphics) -> Self {
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

        Data {
            input_state,
            recipes,
            gl,
            space,
        }
    }
}

type State = dyn GameState<Data, GlGraphics>;

pub struct Playing;

impl Playing {
    fn draw(&mut self, data: &mut Data, args: RenderArgs) {
        let gl = &mut data.gl;
        let ctx = gl.draw_begin(args.viewport());

        graphics::clear(BG_COLOR, gl);

        data.space.run_system(ShapeRenderer::new(&ctx, gl));

        gl.draw_end();
    }

    fn update(&mut self, data: &mut Data, _args: UpdateArgs) {
        data.space.run_system(Motion);
    }
}

impl GameState<Data, GlGraphics> for Playing {
    fn on_event(&mut self, data: &mut Data, evt: &Event) -> Option<Box<State>> {
        data.input_state.handle_event(evt);

        if let Some(args) = evt.render_args() {
            self.draw(data, args);
        } else if let Some(args) = evt.update_args() {
            self.update(data, args);
            data.input_state.update_ages();
        } else if let Some(Button::Keyboard(Key::Space)) = evt.press_args() {
            return Some(Box::new(Paused));
        }

        None
    }
}

pub struct Paused;

impl GameState<Data, GlGraphics> for Paused {
    fn on_event(&mut self, _data: &mut Data, evt: &Event) -> Option<Box<State>> {
        if let Some(Button::Keyboard(Key::Space)) = evt.press_args() {
            return Some(Box::new(Playing));
        }

        None
    }
}
