use crate::test_system::{Mover, Position, Velocity};
use moleengine_core::game::GameState;
use moleengine_core::transform::Transform;
use moleengine_ecs::event::*;
use moleengine_ecs::recipe::{parse_into_space, ObjectRecipe, RecipeBook};
use moleengine_ecs::space::{LifecycleEvent, Space};
use moleengine_ecs::storage::VecStorage;
use moleengine_visuals::shape::{Shape, ShapeRenderer};

use graphics::clear;
use opengl_graphics::GlGraphics;
use piston::input::keyboard::Key;
use piston::input::Button;
use piston::input::*;

pub struct Data {
    recipes: RecipeBook,
    gl: GlGraphics,
    space: Space,
    test_counter: i32,
}

#[derive(Clone, Copy)]
pub struct Rotation(f32);
#[derive(Clone, Copy)]
pub struct Printer;
#[derive(Clone, Copy)]
pub struct Placeholder;
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

impl Data {
    pub fn init(gl: GlGraphics) -> Self {
        let mut space = Space::with_capacity(10);
        space
            .add_container::<Shape, VecStorage<_>>()
            .add_container::<Transform, VecStorage<_>>()
            .add_container::<Position, VecStorage<_>>()
            .add_container::<Velocity, VecStorage<_>>()
            .add_container::<Rotation, VecStorage<_>>()
            .add_container::<Printer, VecStorage<_>>()
            .add_container::<Placeholder, VecStorage<_>>()
            .init_stateful_system(Mover::new());

        let mut recipes = RecipeBook::new();

        let mut thingy = ObjectRecipe::new();
        thingy
            .add(Shape::new_square(50.0, [1.0, 1.0, 1.0, 1.0]))
            .add_named_variable("pos", Some(Position { x: 0.0, y: 0.0 }))
            .add_named_variable("vel", None::<Velocity>)
            .add_named_variable("T", None::<Transform>)
            .add_listener(ChainEventListener);

        recipes.add("thingy", thingy.clone());

        let mut other_thingy = ObjectRecipe::new();
        other_thingy
            .add_listener(ChainEventListener)
            .add_named_variable("P", Some(Position { x: 0.0, y: 0.0 }))
            .add(Velocity { x: 1.0, y: 0.5 })
            .add(Transform::new([120.0, 180.0], 0.0, 1.5))
            .add(Shape::new_square(50.0, [1.0, 0.8, 0.2, 1.0]))
            .add_listener(LifecycleListener);

        recipes.add("other", other_thingy);

        /*
        A bunch of currently unuseful stuff I don't want to delete yet

        // this causes a panic because there's no Velocity
        //thingy.apply(&mut space);
        thingy.set_variable(Velocity { x: 1.0, y: 2.0 });
        thingy.apply(&mut space);
        //thingy.start_disabled();
        thingy.set_variable(Position { x: -5.0, y: 2.5 });
        thingy.apply(&mut space);
        thingy.reset_variables();
        thingy.set_variable(Velocity { x: 0.1, y: 0.0 });
        thingy.apply(&mut space);

        let delete_this = ObjectRecipe::new()
            .add_listener(ChainEventListener)
            .add(Position { x: 0.0, y: 0.0 })
            .add(Velocity { x: 1.0, y: 0.5 })
            .add_listener(LifecycleListener)
            .apply(&mut space);

        println!("disabling delete_this");
        space.disable_object(delete_this);

        for i in 1..10 {
            let n = 0.1 * i as f32;
            let o = ObjectBuilder::try_create(&mut space);
            if let Some(o) = o {
                let id = o
                    .with(Shape::new_square(120.0 - 10.0 * i as f64, [n, n, n, n]))
                    .with(Position {
                        x: i as f32,
                        y: -i as f32,
                    })
                    .with(Rotation(i as f32))
                    .with(Placeholder)
                    .with_listener(Box::new(LifecycleListener))
                    .get_id();

                if i % 2 == 0 {
                    // destruction and replacement test
                    space.destroy_object(id);
                }
            }
        }

        println!("enabling delete_this");
        space.enable_object(delete_this);
        println!("destroying delete_this");
        space.destroy_object(delete_this);

        */

        Data {
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

        clear([0.3, 0.7, 0.8, 1.0], gl);
        //let ctx_ = ctx.trans(50.0, 50.0).rot_deg(data.test_counter as f64);

        data.space.run_system(ShapeRenderer::new(&ctx, gl));

        gl.draw_end();
    }

    fn update(&mut self, data: &mut Data, _args: UpdateArgs) {
        data.test_counter += 1;

        data.space.run_stateful_system::<Mover>();
    }
}

impl GameState<Data, GlGraphics> for Playing {
    fn on_event(&mut self, data: &mut Data, evt: &Event) -> Option<Box<State>> {
        if let Some(args) = evt.render_args() {
            self.draw(data, args);
        } else if let Some(args) = evt.update_args() {
            self.update(data, args);
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
