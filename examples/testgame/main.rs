#[macro_use]
extern crate microprofile;

mod controls;
mod recipes;
mod states;
mod test_events;

//

use glium::glutin;
use moleengine::{ecs, util::InputCache, visuals_glium as vis};

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

    let space = ecs::Space::with_capacity(1000);

    Resources {
        events,
        input_cache,
        space,
    }
}
