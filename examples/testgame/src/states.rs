use moleengine_core::game::GameState;
use moleengine_ecs::event::*;
use moleengine_ecs::space::{LifecycleEvent, ObjectBuilder, ObjectRecipe, Space};
use moleengine_ecs::storage::VecStorage;
use moleengine_ecs::system::{Position, Velocity};
use moleengine_visuals::Shape;

use graphics::{clear, Transformed};
use opengl_graphics::GlGraphics;
use piston::input::keyboard::Key;
use piston::input::Button;
use piston::input::*;

pub struct Data {
    gl: GlGraphics,
    test_space: Space,
    test_counter: i32,
}

#[derive(Clone)]
pub struct Rotation(f32);
#[derive(Clone)]
pub struct Printer;
#[derive(Clone)]
pub struct Placeholder;
#[derive(Clone)]
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

#[derive(Clone)]
pub struct TestChainEvent;

impl SpaceEvent for TestChainEvent {
    fn handle(&self, space: &mut Space) {
        space.run_all_listeners(self);
    }
}

#[derive(Clone)]
pub struct ChainEventListener;

impl EventListener<TestChainEvent> for ChainEventListener {
    fn run_listener(&mut self, _evt: &TestChainEvent, _queue: &mut EventQueue) {
        println!("Chain event");
    }
}

impl Data {
    pub fn init(gl: GlGraphics) -> Self {
        let mut space = Space::with_capacity(10)
            .with_container::<Shape, VecStorage<_>>()
            .with_container::<Position, VecStorage<_>>()
            .with_container::<Velocity, VecStorage<_>>()
            .with_container::<Rotation, VecStorage<_>>()
            .with_container::<Printer, VecStorage<_>>()
            .with_container::<Placeholder, VecStorage<_>>();

        let mut thingy = ObjectRecipe::new();
        thingy
            .add(Shape::new_square(50.0, [1.0, 1.0, 1.0, 1.0]))
            .add(Position { x: 0.0, y: 0.0 })
            .add(Velocity { x: 1.0, y: 0.5 })
            .add_listener(ChainEventListener);

        thingy.apply(&mut space);
        thingy.apply(&mut space);
        thingy.start_disabled();
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

        Data {
            gl: gl,
            test_space: space,
            test_counter: 0,
        }
    }
}

type State = dyn GameState<Data, GlGraphics>;

pub struct Playing;

impl Playing {
    fn draw(&mut self, data: &mut Data, args: &RenderArgs) {
        let gl = &mut data.gl;
        let ctx = gl.draw_begin(args.viewport());

        clear([0.3, 0.7, 0.8, 1.0], gl);
        let _ctx_ = ctx.trans(50.0, 50.0).rot_deg(data.test_counter as f64);

        gl.draw_end();
    }

    fn update(&mut self, data: &mut Data, _args: &UpdateArgs) {
        data.test_counter = data.test_counter + 1;

        data.test_space
            .run_system::<moleengine_ecs::system::Mover>();
    }
}

impl GameState<Data, GlGraphics> for Playing {
    fn on_event(&mut self, data: &mut Data, evt: &Event) -> Option<Box<State>> {
        if let Some(args) = evt.render_args() {
            self.draw(data, &args);
        } else if let Some(args) = evt.update_args() {
            self.update(data, &args);
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
