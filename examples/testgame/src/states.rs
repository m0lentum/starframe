use moleengine_core::game::GameState;
use moleengine_ecs::storage::VecStorage;
use moleengine_ecs::system::{Position, Velocity};
use moleengine_ecs::Space;
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

pub struct Rotation(f32);

pub struct Printer();

pub struct Placeholder();

impl Data {
    pub fn init(gl: GlGraphics) -> Self {
        let mut space = Space::with_capacity(100)
            .with_container::<Shape, VecStorage<_>>()
            .with_container::<Position, VecStorage<_>>()
            .with_container::<Velocity, VecStorage<_>>()
            .with_container::<Rotation, VecStorage<_>>()
            .with_container::<Printer, VecStorage<_>>()
            .with_container::<Placeholder, VecStorage<_>>();

        space
            .create_object()
            .with(Shape::new_square(50.0, [1.0, 1.0, 1.0, 1.0]))
            .with(Position { x: 0.0, y: 0.0 })
            .with(Velocity { x: 1.0, y: 0.5 });

        for i in 1..10 {
            let n = 0.1 * i as f32;
            let o = space
                .create_object()
                .with(Shape::new_square(120.0 - 10.0 * i as f64, [n, n, n, n]))
                .with(Position {
                    x: i as f32,
                    y: -i as f32,
                })
                .with(Rotation(i as f32))
                .with(Placeholder());

            if i % 4 == 0 {
                o.with(Printer());
            }

            // pad the space with some empty objects to verify that component iteration works correctly
            space.create_object();
        }

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

        //let _shapes = (data.test_space.open::<Shape>(),);

        gl.draw_end();
    }

    fn update(&mut self, data: &mut Data, _args: &UpdateArgs) {
        data.test_counter = data.test_counter + 1;

        data.test_space
            .run_system::<moleengine_ecs::system::Mover<'_>>();
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
