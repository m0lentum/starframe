use crate::controls::*;

use moleengine::ecs::{
    event::*,
    recipe::{parse_into_space, ObjectRecipe, RecipeBook},
    space::{LifecycleEvent, Space},
    storage::VecStorage,
};
use moleengine::game::GameState;
use moleengine::util::{debug::*, inputstate::*, Transform};
use moleengine_physics::{collision::RigidBodySolver, systems::Motion, Collider, RigidBody};
use moleengine_visuals::shape::{Shape, ShapeRenderer};

use opengl_graphics::GlGraphics;
use piston::input::keyboard::Key;
use piston::input::Button;
use piston::input::*;

use nalgebra::Point2;

#[derive(Clone, Copy)]
pub struct LifecycleListener;

impl EventListener<LifecycleEvent> for LifecycleListener {
    fn run_listener(&mut self, evt: &LifecycleEvent, queue: &mut EventQueue) {
        match evt {
            LifecycleEvent::Destroy(id) => println!("Object got deleted: {}!", id),
            LifecycleEvent::Disable(id) => println!("Object got disabled: {}!", id),
            LifecycleEvent::Enable(id) => println!("Object got enabled: {}!", id),
        }

        queue.push(Box::new(TestChainEvent));
    }
}

#[derive(Clone, Copy)]
pub struct TestChainEvent;

impl SpaceEvent for TestChainEvent {
    fn handle(&self, space: &mut Space) {
        space.run_all_listeners(self);
    }
}

#[derive(Clone, Copy)]
pub struct ChainEventListener;

impl EventListener<TestChainEvent> for ChainEventListener {
    fn run_listener(&mut self, _evt: &TestChainEvent, _queue: &mut EventQueue) {
        println!("Chain event");
    }
}

pub struct Data {
    input_state: InputState,
    recipes: RecipeBook,
    gl: GlGraphics,
    space: Space,
    test_counter: i32,
}

impl Data {
    pub fn init(gl: GlGraphics) -> Self {
        let mut input_state = InputState::new();
        {
            use Key::*;
            input_state.track_keys(&[Left, Right, Down, Up]);
        }

        let mut space = Space::with_capacity(100);
        space
            .add_container::<Shape, VecStorage<_>>()
            .add_container::<Transform, VecStorage<_>>()
            .add_container::<Collider, VecStorage<_>>()
            .add_container::<RigidBody, VecStorage<_>>()
            .add_container::<KeyboardControls, VecStorage<_>>()
            .add_container::<PointVisualizer, VecStorage<_>>()
            .init_global_state(vec![Point2::new(100_f32, 200_f32)]);

        let mut recipes = RecipeBook::new();

        let coll = Collider::new_circle(20.0);
        let thingy = ObjectRecipe::new()
            .add(Shape::new(coll.as_points(), [1.0, 1.0, 1.0, 1.0]))
            .add(coll)
            .add(RigidBody::new())
            .add_named_variable("T", None::<Transform>)
            .add_listener(ChainEventListener);
        recipes.add("thingy", thingy.clone());

        let other_thingy = ObjectRecipe::new()
            .add_listener(ChainEventListener)
            .add(Transform::new([120.0, 180.0], 0.0, 1.5))
            .add(Shape::new(coll.as_points(), [0.5, 1.0, 0.2, 1.0]))
            .add(coll)
            .add(RigidBody::new())
            .add(KeyboardControls)
            .add_listener(LifecycleListener);
        recipes.add("other", other_thingy);

        let coll_vis = ObjectRecipe::new()
            .add(Transform::identity())
            .add(PointVisualizer)
            .add(Shape::new_square(8.0, [0.1, 0.5, 0.4, 1.0]))
            .start_disabled();
        recipes.add("cv", coll_vis);

        Data {
            input_state,
            recipes,
            gl,
            space,
            test_counter: 0,
        }
    }

    pub fn reload_space(&mut self) {
        let mes = std::fs::read_to_string("./examples/testgame/src/test_space.mes")
            .expect("File read failed");

        self.space.destroy_all();

        let r = parse_into_space(mes.as_str(), &mut self.space, &mut self.recipes);

        let coll_vis = self.recipes.get_mut("cv").unwrap();
        for _ in 1..10 {
            coll_vis.apply(&mut self.space);
        }

        match r {
            Ok(_) => (),
            Err(e) => eprintln!("Error parsing space: {}", e),
        }
    }
}

type State = dyn GameState<Data, GlGraphics>;

pub struct Playing;

impl Playing {
    fn draw(&mut self, data: &mut Data, args: RenderArgs) {
        let gl = &mut data.gl;
        let ctx = gl.draw_begin(args.viewport());

        graphics::clear([0.3, 0.7, 0.8, 1.0], gl);

        data.space.run_system(ShapeRenderer::new(&ctx, gl));

        gl.draw_end();
    }

    fn update(&mut self, data: &mut Data, _args: UpdateArgs) {
        data.test_counter += 1;

        data.space.run_system(KeyboardMover::new(&data.input_state));
        //data.space.run_system(Gravity::down(0.2));
        data.space.run_stateful_system(RigidBodySolver);
        data.space.run_stateful_system(PointVisualizerSystem);
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
        } else if let Some(Button::Keyboard(Key::Return)) = evt.press_args() {
            data.reload_space();
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
