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
    ecs::{self, storage::VecStorage},
    physics2d as phys,
    util::{InputCache, Transform},
    visuals_glium as vis,
};

//

pub struct Resources {
    pub events: glutin::EventsLoop,
    pub input_cache: InputCache,
    pub space: ecs::Space,
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
    let events = unsafe { vis::Context::init() };

    let mut input_cache = InputCache::new();
    {
        use glutin::VirtualKeyCode::*;
        input_cache.track_keys(&[
            Left, Right, Down, Up, PageDown, PageUp, Escape, Return, Space, S, LShift,
        ]);
    }

    let mut space = ecs::Space::with_capacity(1000);
    space.add_container::<vis::Shape, VecStorage<_>>();
    space.add_container::<Transform, VecStorage<_>>();
    space.add_container::<phys::RigidBody, VecStorage<_>>();
    space.add_container::<KeyboardControls, VecStorage<_>>();

    Resources {
        events,
        input_cache,
        space,
    }
}
