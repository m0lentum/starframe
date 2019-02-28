use crate::controls::*;

use moleengine::ecs::{
    event::*,
    recipe::{parse_into_space, ObjectRecipe, RecipeBook},
    space::{LifecycleEvent, Space},
    storage::VecStorage,
};
use moleengine::util::{inputstate::*, Transform};
use moleengine_physics::{
    collision::{Collision, RigidBodySolver},
    systems::Motion,
    Collider, RigidBody,
};
use moleengine_visuals::shape::{Shape, ShapeRenderer};

use opengl_graphics::{GlGraphics, OpenGL};
use piston::event_loop::*;
use piston::input::keyboard::Key;
use piston::input::Button;
use piston::input::*;
use piston_window::{PistonWindow, WindowSettings};

const BG_COLOR: [f32; 4] = [0.1, 0.1, 0.1, 1.0];
const _CYAN_COLOR: [f32; 4] = [0.3, 0.7, 0.8, 1.0];
const LINE_COLOR: [f32; 4] = [0.729, 0.855, 0.333, 1.0];

#[derive(Clone, Copy)]
pub struct LifecycleListener;

impl EventListener<LifecycleEvent> for LifecycleListener {
    fn run_listener(&mut self, evt: &LifecycleEvent, _space: &Space, queue: &mut EventQueue) {
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
    fn run_listener(&mut self, _evt: &TestChainEvent, _space: &Space, _queue: &mut EventQueue) {
        println!("Chain event");
    }
}

#[derive(Clone, Copy)]
pub struct TestCollisionListener;

impl EventListener<Collision> for TestCollisionListener {
    fn run_listener(&mut self, evt: &Collision, space: &Space, _q: &mut EventQueue) {
        space.do_with_component_mut(evt.source, |tr: &mut Transform| {
            tr.rotate_deg(2.0);
        });
    }
}

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
    let events = Events::new(EventSettings::new().ups(60).max_fps(60));

    let mut input_state = InputState::new();
    {
        use Key::*;
        input_state.track_keys(&[Left, Right, Down, Up, PageDown, PageUp]);
    }

    let mut space = Space::with_capacity(100);
    space
        .add_container::<Shape, VecStorage<_>>()
        .add_container::<Transform, VecStorage<_>>()
        .add_container::<Collider, VecStorage<_>>()
        .add_container::<RigidBody, VecStorage<_>>()
        .add_container::<KeyboardControls, VecStorage<_>>()
        .init_global_state::<Vec<Collision>>(Vec::new());

    let mut recipes = RecipeBook::new();

    let coll = Collider::new_rect(180.0, 100.0);
    let thingy = ObjectRecipe::new()
        .add(Shape::new_outlined(
            coll.as_points(),
            [0.0; 4],
            1.0,
            LINE_COLOR,
        ))
        .add(coll)
        .add(RigidBody::new())
        .add_named_variable("T", None::<Transform>)
        //.add_listener(TestCollisionListener)
        .add_listener(ChainEventListener);
    recipes.add("thingy", thingy.clone());

    let other_thingy = ObjectRecipe::new()
        .add_listener(ChainEventListener)
        .add(Transform::new([120.0, 180.0], 0.0, 1.0))
        .add(Shape::new_outlined(
            coll.as_points(),
            [0.0; 4],
            1.0,
            LINE_COLOR,
        ))
        .add(coll)
        .add(RigidBody::new())
        .add(KeyboardControls)
        .add_listener(LifecycleListener);
    recipes.add("other", other_thingy);

    Resources {
        gl,
        window,
        events,
        input_state,
        recipes,
        space,
    }
}

pub fn reload_space(space: &mut Space, recipes: &mut RecipeBook) {
    let mes = std::fs::read_to_string("./examples/testgame/src/test_space.mes")
        .expect("File read failed");

    space.destroy_all();

    let r = parse_into_space(mes.as_str(), space, recipes);

    match r {
        Ok(_) => (),
        Err(e) => eprintln!("Error parsing space: {}", e),
    }
}

fn draw_space(gl: &mut GlGraphics, args: RenderArgs, space: &mut Space) {
    let ctx = gl.draw_begin(args.viewport());

    graphics::clear(BG_COLOR, gl);

    space.run_system(ShapeRenderer::new(&ctx, gl));
    moleengine_physics::collision::debug::draw_collisions(&space, &ctx, gl, [1.0, 0.3, 0.2, 1.0]);

    gl.draw_end();
}

fn update_playing(res: &mut Resources, _args: UpdateArgs) {
    res.space.run_system(KeyboardMover::new(&res.input_state));
    res.space.run_stateful_system(RigidBodySolver);
    res.space.run_system(Motion);
}

pub fn begin(mut res: Resources) {
    state_playing(&mut res);
}

fn state_playing(res: &mut Resources) {
    while let Some(evt) = res.events.next(&mut res.window) {
        res.input_state.handle_event(&evt);

        if let Some(args) = evt.render_args() {
            draw_space(&mut res.gl, args, &mut res.space);
        } else if let Some(args) = evt.update_args() {
            update_playing(res, args);
            res.input_state.update_ages();
        } else if let Some(Button::Keyboard(Key::Space)) = evt.press_args() {
            state_paused(res);
        } else if let Some(Button::Keyboard(Key::Return)) = evt.press_args() {
            reload_space(&mut res.space, &mut res.recipes);
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
