#[macro_use]
extern crate microprofile;

mod states;

//

use glium::glutin;
use moleengine::{
    core, physics2d as phys,
    util::{InputCache, Transform},
    visuals_glium::{self as vis, camera as cam},
};

//

pub type Camera = cam::Camera2D<cam::MouseDragController>;

pub struct MainSpaceFeatures {
}

impl core::space::FeatureSet for MainSpaceFeatures {
    fn init(capacity: core::space::IdType) -> Self {
        MainSpaceFeatures {}
    }
    fn tick(dt: f32) {
    }
}

type MainSpace = core::Space<MainSpaceFeatures>;

pub struct Resources {
    pub events: glutin::EventsLoop,
    pub space: MainSpace,
    pub camera: Camera,
    pub input_cache: InputCache,
    pub impulse_cache: phys::constraint::ImpulseCache, // TODO: this can now exist inside a spacefeature
}

fn main() {
    microprofile::init!();
    microprofile::set_enable_all_groups!(true);

    let res = init_resources();
    // states::begin(res);

    //microprofile::dump_file_immediately!("profile.html", "");
    microprofile::shutdown!();
}

pub fn init_resources() -> Resources {
    let events = unsafe { vis::Context::init() };

    let mut input_cache = InputCache::new();
    {
        use glutin::VirtualKeyCode::*;
        input_cache.track_keys(&[
            Left, Right, Down, Up, PageDown, PageUp, Escape, Return, Space, S, T, LShift,
        ]);
    }

    let space = core::Space::with_capacity(1000);

    let camera = cam::Camera2D::new(
        cam::MouseDragController::new(Transform::identity()),
        vis::camera::ScalingStrategy::ConstantDisplayArea {
            width: 8.0,
            height: 6.0,
        },
    );

    let impulse_cache = phys::constraint::ImpulseCache::new();

    //

    let contact_cache = phys::collision::ContactOutput::new();

    Resources {
        events,
        space,
        camera,
        input_cache,
        impulse_cache,
    }
}
