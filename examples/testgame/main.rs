#[macro_use]
extern crate microprofile;

mod controls;
mod recipes;
mod states;
mod test_events;

//

use self::controls::KeyboardControls;
use glium::glutin;
use moleengine::{
    ecs::{space::Space, storage::VecStorage},
    physics2d::{Collider, CollisionEvent, RigidBody},
    util::{inputcache::InputCache, Transform},
    visuals_glium::{debug::IntersectionIndicator, shaders::Shaders, shape::Shape},
};

//

pub struct Resources {
    pub display: glium::Display,
    pub events: glutin::EventsLoop,
    pub shaders: Shaders,
    pub input_cache: InputCache,
    pub space: Space,
    pub intersection_vis: IntersectionIndicator,
}

fn main() {
    microprofile::init!();
    microprofile::set_enable_all_groups!(true);

    let res = init_resources();
    states::begin(res);

    //microprofile::dump_file_immediately!("profile.html", "");
    microprofile::shutdown!();
}

pub fn init_resources() -> Resources {
    let events = glutin::EventsLoop::new();
    let window = glutin::WindowBuilder::new()
        .with_title("MoleEngine test")
        .with_dimensions(glutin::dpi::LogicalSize::new(800.0, 600.0));
    let context = glutin::ContextBuilder::new();
    let display = glium::Display::new(window, context, &events).expect("Failed to create display");

    let shaders = Shaders::compile(&display).expect("Failed to compile shader");

    let mut input_cache = InputCache::new();
    {
        use glutin::VirtualKeyCode::*;
        input_cache.track_keys(&[
            Left, Right, Down, Up, PageDown, PageUp, Escape, Return, Space, S, LShift,
        ]);
    }

    let mut space = Space::with_capacity(100);
    space
        .add_container::<Shape, VecStorage<_>>()
        .add_container::<Transform, VecStorage<_>>()
        .add_container::<Collider, VecStorage<_>>()
        .add_container::<RigidBody, VecStorage<_>>()
        .add_container::<KeyboardControls, VecStorage<_>>()
        .init_global_state::<Vec<CollisionEvent>>(Vec::new());

    let intersection_vis = IntersectionIndicator::new(&display, 50);

    Resources {
        display,
        events,
        shaders,
        input_cache,
        space,
        intersection_vis,
    }
}
